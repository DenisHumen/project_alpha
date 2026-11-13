// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
)

// LsblkColumns is the exact column list passed to `lsblk -o`.
// The parser also accepts output of older util-linux versions (< 2.37),
// where booleans and sizes are strings and MOUNTPOINTS is MOUNTPOINT.
const LsblkColumns = "NAME,KNAME,PATH,TYPE,MODEL,VENDOR,SERIAL,WWN,SIZE,TRAN,ROTA,RM,HOTPLUG,RO," +
	"PTTYPE,PARTTYPE,PARTLABEL,FSTYPE,LABEL,UUID,MOUNTPOINTS"

// LsblkArgs are the arguments for the read-only lsblk invocation (full tree, bytes, JSON).
var LsblkArgs = []string{"-J", "-b", "-o", LsblkColumns}

// lsblkDevice is one node of `lsblk -J` output.
type lsblkDevice struct {
	Name        string        `json:"name"`
	KName       string        `json:"kname"`
	Path        string        `json:"path"`
	Type        string        `json:"type"`
	Model       string        `json:"model"`
	Vendor      string        `json:"vendor"`
	Serial      string        `json:"serial"`
	WWN         string        `json:"wwn"`
	Size        flexUint      `json:"size"`
	Tran        string        `json:"tran"`
	Rota        flexBool      `json:"rota"`
	RM          flexBool      `json:"rm"`
	Hotplug     flexBool      `json:"hotplug"`
	RO          flexBool      `json:"ro"`
	PTType      string        `json:"pttype"`
	PartType    string        `json:"parttype"`
	PartLabel   string        `json:"partlabel"`
	FSType      string        `json:"fstype"`
	Label       string        `json:"label"`
	UUID        string        `json:"uuid"`
	Mountpoints []*string     `json:"mountpoints"`
	Mountpoint  *string       `json:"mountpoint"` // util-linux < 2.37
	Children    []lsblkDevice `json:"children"`
}

type lsblkOutput struct {
	BlockDevices []lsblkDevice `json:"blockdevices"`
}

// mounts returns the non-empty mountpoints of this node (both column variants).
func (d *lsblkDevice) mounts() []string {
	var out []string
	for _, m := range d.Mountpoints {
		if m != nil && *m != "" {
			out = append(out, *m)
		}
	}
	if d.Mountpoint != nil && *d.Mountpoint != "" && !contains(out, *d.Mountpoint) {
		out = append(out, *d.Mountpoint)
	}
	return out
}

// devPath returns PATH, or /dev/NAME when the PATH column is absent (old lsblk).
func (d *lsblkDevice) devPath() string {
	if d.Path != "" {
		return d.Path
	}
	return "/dev/" + d.Name
}

func parseLsblk(data []byte) ([]lsblkDevice, error) {
	var out lsblkOutput
	if err := json.Unmarshal(data, &out); err != nil {
		return nil, fmt.Errorf("parse lsblk json: %w", err)
	}
	return out.BlockDevices, nil
}

// flexBool accepts true/false, 0/1, "0"/"1", "true"/"false" and null.
type flexBool bool

func (b *flexBool) UnmarshalJSON(data []byte) error {
	s := strings.Trim(string(bytes.TrimSpace(data)), `"`)
	switch strings.ToLower(s) {
	case "1", "true":
		*b = true
	case "0", "false", "", "null":
		*b = false
	default:
		return fmt.Errorf("unexpected boolean %q", s)
	}
	return nil
}

// flexUint accepts a JSON number, a numeric string and null.
type flexUint uint64

func (u *flexUint) UnmarshalJSON(data []byte) error {
	s := strings.Trim(string(bytes.TrimSpace(data)), `"`)
	if s == "" || s == "null" {
		*u = 0
		return nil
	}
	v, err := strconv.ParseUint(s, 10, 64)
	if err != nil {
		return fmt.Errorf("unexpected size %q (lsblk must run with -b): %w", s, err)
	}
	*u = flexUint(v)
	return nil
}

func contains(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}
