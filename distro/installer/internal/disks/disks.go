// SPDX-License-Identifier: MPL-2.0

// Package disks discovers the block devices the installer may offer as the
// system disk. It is strictly read-only: it runs `lsblk`, `udevadm info` and
// (optionally) `smartctl -H`, and reads sysfs/procfs, but never opens a block
// device for writing and never changes partition tables or filesystems.
package disks

import (
	"fmt"
	"sort"
	"strconv"
	"strings"
)

// Transport is the bus a disk is attached to.
type Transport string

const (
	TransportNVMe    Transport = "nvme"
	TransportSATA    Transport = "sata"
	TransportSAS     Transport = "sas"
	TransportSCSI    Transport = "scsi"
	TransportUSB     Transport = "usb"
	TransportMMC     Transport = "mmc"
	TransportVirtio  Transport = "virtio"
	TransportXen     Transport = "xen"
	TransportUnknown Transport = "unknown"
)

// Media is the storage medium as far as it can be inferred.
type Media string

const (
	MediaSSD     Media = "ssd"
	MediaHDD     Media = "hdd"
	MediaFlash   Media = "flash" // USB sticks, SD cards, eMMC
	MediaVirtual Media = "virtual"
	MediaOptical Media = "optical"
	MediaUnknown Media = "unknown"
)

// ExcludeReason says why a disk cannot be the install target.
type ExcludeReason string

const (
	ExcludeNone      ExcludeReason = ""
	ExcludeVirtual   ExcludeReason = "virtual-device" // loop, zram, ram, nbd
	ExcludeOptical   ExcludeReason = "optical"        // sr*, TYPE=rom
	ExcludeBootMedia ExcludeReason = "boot-media"     // the installer's own media or the running system
	ExcludeNoMedia   ExcludeReason = "no-media"       // empty card reader slot
	ExcludeReadOnly  ExcludeReason = "read-only"      // write-protected
	ExcludeTooSmall  ExcludeReason = "too-small"      // below Options.MinSize
	ExcludeHWArea    ExcludeReason = "hardware-area"  // mmcblkXbootN, mmcblkXrpmb
	ExcludeNotDisk   ExcludeReason = "not-a-disk"     // any other TYPE
)

// Partition is a child of a disk as reported by lsblk.
type Partition struct {
	Name        string   `json:"name"`
	Path        string   `json:"path"`
	Size        uint64   `json:"size"`
	FSType      string   `json:"fstype,omitempty"`
	Label       string   `json:"label,omitempty"`
	PartType    string   `json:"parttype,omitempty"`
	PartLabel   string   `json:"partlabel,omitempty"`
	Mountpoints []string `json:"mountpoints,omitempty"`
}

// Disk is a whole block device with everything the install screen needs.
type Disk struct {
	Name          string        `json:"name"`      // nvme0n1
	Path          string        `json:"path"`      // /dev/nvme0n1
	Model         string        `json:"model"`     // human name: "Samsung 990 PRO 1TB"
	RawModel      string        `json:"raw_model"` // as reported: "Samsung SSD 990 PRO 1TB"
	Vendor        string        `json:"vendor,omitempty"`
	Serial        string        `json:"serial,omitempty"`
	WWN           string        `json:"wwn,omitempty"`
	ByPath        string        `json:"by_path,omitempty"` // udev ID_PATH: stable slot identifier
	Size          uint64        `json:"size"`              // bytes
	Transport     Transport     `json:"transport"`
	Media         Media         `json:"media"`
	Rotational    bool          `json:"rotational"`
	Removable     bool          `json:"removable"`
	Hotplug       bool          `json:"hotplug"`
	ReadOnly      bool          `json:"read_only"`
	PartTable     string        `json:"pttype,omitempty"`
	FSType        string        `json:"fstype,omitempty"` // filesystem/signature on the whole disk
	FSLabel       string        `json:"label,omitempty"`
	Partitions    []Partition   `json:"partitions,omitempty"`
	Findings      []Finding     `json:"findings,omitempty"`
	Smart         *SmartHealth  `json:"smart,omitempty"`
	Excluded      bool          `json:"excluded"`
	ExcludeReason ExcludeReason `json:"exclude_reason,omitempty"`
}

// SizeString is the decimal size as printed on the box ("1.0 TB").
func (d Disk) SizeString() string { return FormatDecimal(d.Size) }

// Label is the one-line name for lists: "Samsung 990 PRO 1TB — /dev/nvme0n1".
func (d Disk) Label() string { return d.Model + " — " + d.Path }

// HasData reports whether anything on the disk would be destroyed by installing.
func (d Disk) HasData() bool {
	for _, f := range d.Findings {
		if f.Kind.destructive() {
			return true
		}
	}
	return false
}

// KindLabel is the short bus/medium label for the list: "NVMe", "SATA SSD", "HDD", "USB".
func (d Disk) KindLabel() string {
	if d.Media == MediaOptical {
		return "optical"
	}
	switch d.Transport {
	case TransportNVMe:
		return "NVMe"
	case TransportUSB:
		return "USB"
	case TransportMMC:
		if strings.HasPrefix(d.Name, "mmcblk") && !d.Removable {
			return "eMMC"
		}
		return "SD/MMC"
	case TransportVirtio:
		return "virtio"
	case TransportXen:
		return "Xen"
	}
	if d.Media == MediaVirtual {
		if d.Transport == TransportUnknown {
			return "virtual"
		}
		return "virtual " + strings.ToUpper(string(d.Transport))
	}
	bus := strings.ToUpper(string(d.Transport))
	if d.Transport == TransportUnknown {
		bus = ""
	}
	switch d.Media {
	case MediaHDD:
		return strings.TrimSpace(bus + " HDD")
	case MediaSSD:
		return strings.TrimSpace(bus + " SSD")
	}
	if bus == "" {
		return "disk"
	}
	return bus
}

// MaskedSerial shows only the tail of the serial number ("…1234"), as on the install screen.
func (d Disk) MaskedSerial() string {
	s := strings.TrimSpace(d.Serial)
	if s == "" || s == redacted {
		return ""
	}
	r := []rune(s)
	if len(r) <= 4 {
		return "…" + s
	}
	return "…" + string(r[len(r)-4:])
}

// Options tune Probe.
type Options struct {
	// MinSize is the smallest disk (bytes) accepted as an install target.
	// Zero means DefaultMinSize.
	MinSize uint64
	// Smart runs `smartctl -H -j` for every candidate disk.
	Smart bool
}

// DefaultMinSize is the provisional minimum system disk size (16 GB):
// ESP 1 GiB + /boot 2 GiB + root with room for two OS deployments.
// DIST-005 owns the final number.
const DefaultMinSize = 16_000_000_000

// Probe lists whole disks with their human names, flags and findings.
// Excluded disks are returned too (Excluded=true) so the UI can explain
// why something is missing; use Candidates to get only installable ones.
func Probe(src Source, opt Options) ([]Disk, error) {
	if opt.MinSize == 0 {
		opt.MinSize = DefaultMinSize
	}
	raw, err := src.Lsblk()
	if err != nil {
		return nil, fmt.Errorf("lsblk: %w", err)
	}
	nodes, err := parseLsblk(raw)
	if err != nil {
		return nil, err
	}
	boot := bootHints(src)

	out := make([]Disk, 0, len(nodes))
	for i := range nodes {
		n := &nodes[i]
		d := newDisk(n)
		enrich(&d, n, src)
		d.Findings = findings(n)
		if r := excludeReason(&d, n, boot, opt.MinSize); r != ExcludeNone {
			d.Excluded, d.ExcludeReason = true, r
		}
		if opt.Smart && !d.Excluded {
			if h, err := smartHealth(src, d.Path); err == nil && h != nil {
				d.Smart = h
				if h.Available && !h.Passed {
					d.Findings = append(d.Findings, Finding{Kind: FindingSmartFailed, Device: d.Path})
				}
			}
		}
		out = append(out, d)
	}
	sortDisks(out)
	return out, nil
}

// Candidates filters out excluded disks.
func Candidates(all []Disk) []Disk {
	var out []Disk
	for _, d := range all {
		if !d.Excluded {
			out = append(out, d)
		}
	}
	return out
}

func newDisk(n *lsblkDevice) Disk {
	d := Disk{
		Name:       n.Name,
		Path:       n.devPath(),
		RawModel:   collapseSpaces(n.Model),
		Vendor:     collapseSpaces(n.Vendor),
		Serial:     strings.TrimSpace(n.Serial),
		WWN:        strings.TrimSpace(n.WWN),
		Size:       uint64(n.Size),
		Rotational: bool(n.Rota),
		Removable:  bool(n.RM),
		Hotplug:    bool(n.Hotplug),
		ReadOnly:   bool(n.RO),
		PartTable:  n.PTType,
		FSType:     n.FSType,
		FSLabel:    n.Label,
	}
	for _, c := range n.Children {
		if c.Type != "part" {
			continue
		}
		d.Partitions = append(d.Partitions, Partition{
			Name: c.Name, Path: c.devPath(), Size: uint64(c.Size),
			FSType: c.FSType, Label: c.Label, PartType: strings.ToLower(c.PartType),
			PartLabel: c.PartLabel, Mountpoints: c.mounts(),
		})
	}
	return d
}

// enrich fills model, transport and medium from lsblk, sysfs and udev.
func enrich(d *Disk, n *lsblkDevice, src Source) {
	var udev map[string]string
	if n.Type == "disk" && !isVirtualName(n.Name) {
		udev, _ = src.Udev(d.Path) // best effort: missing udev data only costs precision
	}

	// NVMe: the controller model in sysfs is complete; lsblk may truncate or omit it.
	if strings.HasPrefix(n.Name, "nvme") {
		if b, err := src.ReadFile("sys/block/" + n.Name + "/device/model"); err == nil {
			if m := collapseSpaces(string(b)); m != "" {
				d.RawModel = m
			}
		}
	}
	if d.RawModel == "" && udev != nil {
		if enc := udev["ID_MODEL_ENC"]; enc != "" {
			d.RawModel = collapseSpaces(decodeUdevEnc(enc))
		} else if m := udev["ID_MODEL"]; m != "" {
			d.RawModel = collapseSpaces(strings.ReplaceAll(m, "_", " "))
		}
	}
	if d.Vendor == "" && udev != nil {
		if enc := udev["ID_VENDOR_ENC"]; enc != "" {
			d.Vendor = collapseSpaces(decodeUdevEnc(enc))
		} else {
			d.Vendor = collapseSpaces(udev["ID_VENDOR"])
		}
	}
	if d.Serial == "" && udev != nil {
		d.Serial = udev["ID_SERIAL_SHORT"]
	}
	if udev != nil {
		d.ByPath = udev["ID_PATH"]
	}

	if n.Type == "loop" || isVirtualName(n.Name) {
		d.Transport, d.Media = TransportUnknown, MediaVirtual
		d.Model = strings.TrimRight(n.Name, "0123456789") + " device"
		return
	}
	d.Transport = detectTransport(n, udev)
	d.Media = detectMedia(d)
	if n.Type == "rom" {
		d.Media = MediaOptical
	}
	d.Model = HumanName(d.RawModel, d.Vendor)
	if d.Model == "" {
		d.Model = fallbackName(d)
	}
}

func detectTransport(n *lsblkDevice, udev map[string]string) Transport {
	switch strings.ToLower(n.Tran) {
	case "nvme":
		return TransportNVMe
	case "sata", "ata":
		return TransportSATA
	case "sas":
		return TransportSAS
	case "usb":
		return TransportUSB
	case "mmc":
		return TransportMMC
	case "virtio":
		return TransportVirtio
	case "spi", "fc", "iscsi", "ieee1394":
		return TransportSCSI
	}
	path := udev["ID_PATH"]
	switch {
	case udev["ID_BUS"] == "usb" || strings.Contains(path, "-usb-"):
		return TransportUSB
	case strings.HasPrefix(n.Name, "nvme"):
		return TransportNVMe
	case strings.HasPrefix(n.Name, "vd") || strings.Contains(path, "virtio"):
		return TransportVirtio
	case strings.HasPrefix(n.Name, "xvd"):
		return TransportXen
	case strings.HasPrefix(n.Name, "mmcblk"):
		return TransportMMC
	case udev["ID_BUS"] == "ata":
		return TransportSATA
	case udev["ID_BUS"] == "scsi":
		return TransportSCSI
	}
	return TransportUnknown
}

var virtualModelPrefixes = []string{"QEMU", "VBOX", "VMWARE", "VIRTUAL", "MSFT VIRTUAL"}

func detectMedia(d *Disk) Media {
	switch d.Transport {
	case TransportVirtio, TransportXen:
		return MediaVirtual
	case TransportMMC:
		return MediaFlash
	}
	up := strings.ToUpper(d.RawModel)
	for _, p := range virtualModelPrefixes {
		if strings.HasPrefix(up, p) || strings.HasPrefix(strings.ToUpper(d.Vendor), p) {
			return MediaVirtual
		}
	}
	switch d.Transport {
	case TransportNVMe:
		return MediaSSD
	case TransportUSB:
		// USB bridges report ROTA=1 for flash sticks too: do not guess.
		return MediaUnknown
	}
	if d.Rotational {
		return MediaHDD
	}
	return MediaSSD
}

// fallbackName is used when there is no model at all (virtio, some USB bridges):
// vendor (if meaningful) + kind + size, e.g. "Virtual disk 64 GB".
func fallbackName(d *Disk) string {
	noun := "Disk"
	switch d.Transport {
	case TransportVirtio, TransportXen:
		noun = "Virtual disk"
	case TransportUSB:
		noun = "USB drive"
	case TransportMMC:
		noun = "Memory card"
		if !d.Removable {
			noun = "eMMC"
		}
	}
	name := noun + " " + d.SizeString()
	if v := d.Vendor; v != "" && !genericVendors[strings.ToUpper(v)] && !strings.HasPrefix(strings.ToUpper(v), "0X") {
		name = brandOrSelf(v) + " " + name
	}
	return name
}

func isVirtualName(name string) bool {
	for _, p := range []string{"loop", "zram", "ram", "nbd"} {
		if strings.HasPrefix(name, p) {
			return true
		}
	}
	return false
}

func excludeReason(d *Disk, n *lsblkDevice, boot bootInfo, minSize uint64) ExcludeReason {
	switch {
	case n.Type == "loop" || isVirtualName(n.Name):
		return ExcludeVirtual
	case n.Type == "rom" || strings.HasPrefix(n.Name, "sr"):
		return ExcludeOptical
	case n.Type != "disk":
		return ExcludeNotDisk
	case isMMCHardwareArea(n.Name):
		return ExcludeHWArea
	case boot.matches(n):
		return ExcludeBootMedia
	case d.Size == 0:
		return ExcludeNoMedia
	case d.ReadOnly:
		return ExcludeReadOnly
	case d.Size < minSize:
		return ExcludeTooSmall
	}
	return ExcludeNone
}

// isMMCHardwareArea matches eMMC boot partitions and RPMB (mmcblk0boot0, mmcblk0rpmb).
func isMMCHardwareArea(name string) bool {
	if !strings.HasPrefix(name, "mmcblk") {
		return false
	}
	rest := strings.TrimLeft(strings.TrimPrefix(name, "mmcblk"), "0123456789")
	return strings.HasPrefix(rest, "boot") || rest == "rpmb"
}

var transportRank = map[Transport]int{
	TransportNVMe: 0, TransportSATA: 1, TransportSAS: 2, TransportSCSI: 3,
	TransportVirtio: 4, TransportXen: 5, TransportMMC: 6, TransportUSB: 7, TransportUnknown: 8,
}

// sortDisks: candidates first; then NVMe, SATA, SAS…, USB last; then by name.
func sortDisks(ds []Disk) {
	sort.SliceStable(ds, func(i, j int) bool {
		a, b := ds[i], ds[j]
		if a.Excluded != b.Excluded {
			return !a.Excluded
		}
		if transportRank[a.Transport] != transportRank[b.Transport] {
			return transportRank[a.Transport] < transportRank[b.Transport]
		}
		return naturalLess(a.Name, b.Name)
	})
}

// naturalLess orders sdz < sdaa and nvme2n1 < nvme10n1: digit runs compare
// numerically, letter runs by length first, then lexicographically.
func naturalLess(a, b string) bool {
	ca, cb := chunks(a), chunks(b)
	for i := 0; i < len(ca) && i < len(cb); i++ {
		x, y := ca[i], cb[i]
		if x == y {
			continue
		}
		if isDigits(x) && isDigits(y) {
			nx, _ := strconv.ParseUint(x, 10, 64)
			ny, _ := strconv.ParseUint(y, 10, 64)
			if nx != ny {
				return nx < ny
			}
		}
		if len(x) != len(y) {
			return len(x) < len(y)
		}
		return x < y
	}
	return len(ca) < len(cb)
}

func chunks(s string) []string {
	var out []string
	start := 0
	for i := 1; i <= len(s); i++ {
		if i == len(s) || isDigit(s[i]) != isDigit(s[i-1]) {
			out = append(out, s[start:i])
			start = i
		}
	}
	return out
}

func isDigit(c byte) bool { return c >= '0' && c <= '9' }

func isDigits(s string) bool { return s != "" && isDigit(s[0]) }
