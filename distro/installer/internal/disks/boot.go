// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"strings"
)

// systemMounts are mountpoints that belong to the running system. A disk with
// any of them in its subtree is the installer's own boot media (live USB:
// /run/initramfs/live) or the disk the current OS runs from (/, /sysroot…).
var systemMounts = map[string]bool{
	"/run/initramfs/live": true, // dmsquash-live: the ISO/USB filesystem
	"/run/rootfsbase":     true, // dmsquash-live overlay base
	"/":                   true,
	"/sysroot":            true,
	"/boot":               true,
	"/boot/efi":           true,
	"/efi":                true,
	"/usr":                true,
}

// bootInfo holds what identifies the installer's boot media besides mounts:
// the live root from the kernel command line (root=live:CDLABEL=…).
type bootInfo struct {
	labels []string // filesystem labels
	uuids  []string // filesystem UUIDs
	devs   []string // explicit device paths (root=live:/dev/sdb1)
}

// bootHints reads /proc/cmdline through the source. Errors mean "no hints":
// the mountpoint check still works without them.
func bootHints(src Source) bootInfo {
	b, err := src.ReadFile("proc/cmdline")
	if err != nil {
		return bootInfo{}
	}
	return parseCmdline(string(b))
}

func parseCmdline(cmdline string) bootInfo {
	var bi bootInfo
	for _, arg := range strings.Fields(cmdline) {
		val, ok := strings.CutPrefix(arg, "root=live:")
		if !ok {
			continue
		}
		switch {
		case strings.HasPrefix(val, "CDLABEL="):
			bi.labels = append(bi.labels, decodeUdevEnc(strings.TrimPrefix(val, "CDLABEL=")))
		case strings.HasPrefix(val, "LABEL="):
			bi.labels = append(bi.labels, decodeUdevEnc(strings.TrimPrefix(val, "LABEL=")))
		case strings.HasPrefix(val, "UUID="):
			bi.uuids = append(bi.uuids, strings.ToLower(strings.TrimPrefix(val, "UUID=")))
		case strings.HasPrefix(val, "/dev/"):
			bi.devs = append(bi.devs, val)
		}
	}
	return bi
}

// matches reports whether the disk subtree holds the boot media or the running system.
func (bi bootInfo) matches(disk *lsblkDevice) bool {
	found := false
	var visit func(n *lsblkDevice)
	visit = func(n *lsblkDevice) {
		if found {
			return
		}
		for _, m := range n.mounts() {
			if systemMounts[m] {
				found = true
				return
			}
		}
		if n.Label != "" && contains(bi.labels, n.Label) ||
			n.UUID != "" && contains(bi.uuids, strings.ToLower(n.UUID)) ||
			contains(bi.devs, n.devPath()) {
			found = true
			return
		}
		for i := range n.Children {
			visit(&n.Children[i])
		}
	}
	visit(disk)
	return found
}
