// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"fmt"
	"strings"
)

// FindingKind classifies something already present on a disk. The TUI
// localises findings by kind; String() is the English fallback for logs/CLI.
type FindingKind string

const (
	FindingPartitionTable FindingKind = "partition-table" // Detail: gpt|dos, Count: partitions
	FindingFilesystem     FindingKind = "filesystem"      // FSType, Label
	FindingRAIDMember     FindingKind = "raid-member"     // Label: array name ("nas:0"), Detail: md device
	FindingLVMMember      FindingKind = "lvm-member"
	FindingLUKS           FindingKind = "luks"
	FindingZFSMember      FindingKind = "zfs-member" // Label: pool name
	FindingSwap           FindingKind = "swap"
	FindingWindows        FindingKind = "os-windows" // Microsoft reserved partition present
	FindingEFISystem      FindingKind = "os-efi"     // EFI system partition: some OS is (or was) installed
	FindingMounted        FindingKind = "mounted"    // Detail: mountpoint
	FindingSmartFailed    FindingKind = "smart-failed"
)

// Finding is one fact about existing content on a disk.
type Finding struct {
	Kind   FindingKind `json:"kind"`
	Device string      `json:"device"`
	FSType string      `json:"fstype,omitempty"`
	Label  string      `json:"label,omitempty"`
	Detail string      `json:"detail,omitempty"`
	Count  int         `json:"count,omitempty"`
}

func (k FindingKind) destructive() bool {
	return k != FindingSmartFailed
}

func (f Finding) String() string {
	label := ""
	if f.Label != "" {
		label = fmt.Sprintf(" %q", f.Label)
	}
	switch f.Kind {
	case FindingPartitionTable:
		return fmt.Sprintf("%s partition table, %d partition(s)", strings.ToUpper(f.Detail), f.Count)
	case FindingFilesystem:
		return fmt.Sprintf("%s%s on %s", f.FSType, label, f.Device)
	case FindingRAIDMember:
		s := fmt.Sprintf("mdraid member%s on %s", label, f.Device)
		if f.Detail != "" {
			s += " (array " + f.Detail + ")"
		}
		return s
	case FindingLVMMember:
		return "LVM physical volume on " + f.Device
	case FindingLUKS:
		return "LUKS encrypted volume on " + f.Device
	case FindingZFSMember:
		return fmt.Sprintf("ZFS pool member%s on %s", label, f.Device)
	case FindingSwap:
		return "swap on " + f.Device
	case FindingWindows:
		return "Windows installation (Microsoft reserved partition " + f.Device + ")"
	case FindingEFISystem:
		return "EFI system partition " + f.Device + " (an OS is likely installed)"
	case FindingMounted:
		if f.Detail == "[SWAP]" {
			return f.Device + " is in use as swap"
		}
		return f.Device + " is mounted at " + f.Detail
	case FindingSmartFailed:
		return "SMART health check FAILED"
	}
	return string(f.Kind) + " on " + f.Device
}

// GPT partition type GUIDs (lower case, as lsblk prints them).
const (
	guidESP          = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
	guidMSReserved   = "e3c9e316-0b5c-4db8-817d-f92df00215ae"
	mbrESP           = "0xef"
	guidLinuxRAID    = "a19d880f-05fc-4d3b-a006-743f0f84911e"
	mbrLinuxRAID     = "0xfd"
	guidLinuxSwap    = "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f"
	fsTypeRAIDMember = "linux_raid_member"
)

// findings walks a disk's lsblk subtree and records existing content.
func findings(disk *lsblkDevice) []Finding {
	var out []Finding
	if disk.PTType != "" {
		parts := 0
		for _, c := range disk.Children {
			if c.Type == "part" {
				parts++
			}
		}
		out = append(out, Finding{Kind: FindingPartitionTable, Device: disk.devPath(), Detail: disk.PTType, Count: parts})
	}
	// The whole disk itself may carry a signature (raw mdraid member, iso9660, btrfs on the bare disk).
	out = append(out, nodeFindings(disk)...)
	// Partitions and stacked devices (md arrays, dm-crypt, LVM volumes) below them.
	var walk func(n *lsblkDevice)
	walk = func(n *lsblkDevice) {
		for i := range n.Children {
			out = append(out, nodeFindings(&n.Children[i])...)
			walk(&n.Children[i])
		}
	}
	walk(disk)
	return dedupe(out)
}

// dedupe drops repeated findings (an md array below two partitions of one disk).
func dedupe(in []Finding) []Finding {
	seen := map[Finding]bool{}
	out := in[:0]
	for _, f := range in {
		if !seen[f] {
			seen[f] = true
			out = append(out, f)
		}
	}
	return out
}

func nodeFindings(n *lsblkDevice) []Finding {
	var out []Finding
	dev := n.devPath()
	pt := strings.ToLower(n.PartType)
	switch pt {
	case guidESP, mbrESP:
		out = append(out, Finding{Kind: FindingEFISystem, Device: dev})
	case guidMSReserved:
		out = append(out, Finding{Kind: FindingWindows, Device: dev})
	}
	switch n.FSType {
	case "":
		if pt == guidLinuxRAID || pt == mbrLinuxRAID {
			out = append(out, Finding{Kind: FindingRAIDMember, Device: dev})
		}
	case fsTypeRAIDMember:
		f := Finding{Kind: FindingRAIDMember, Device: dev, Label: n.Label}
		for _, c := range n.Children {
			if strings.HasPrefix(c.Type, "raid") {
				f.Detail = c.devPath()
				break
			}
		}
		out = append(out, f)
	case "LVM2_member":
		out = append(out, Finding{Kind: FindingLVMMember, Device: dev})
	case "crypto_LUKS":
		out = append(out, Finding{Kind: FindingLUKS, Device: dev})
	case "zfs_member":
		out = append(out, Finding{Kind: FindingZFSMember, Device: dev, Label: n.Label})
	case "swap":
		out = append(out, Finding{Kind: FindingSwap, Device: dev})
	default:
		if pt != guidESP && pt != mbrESP || n.FSType != "vfat" {
			out = append(out, Finding{Kind: FindingFilesystem, Device: dev, FSType: n.FSType, Label: n.Label})
		}
	}
	for _, m := range n.mounts() {
		out = append(out, mountFinding(n, m))
	}
	return out
}

func mountFinding(n *lsblkDevice, mountpoint string) Finding {
	return Finding{Kind: FindingMounted, Device: n.devPath(), Detail: mountpoint}
}
