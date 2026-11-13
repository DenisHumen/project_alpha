// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// Record saves a snapshot of src into dir in the Dir layout, so a user's
// hardware can be replayed in tests or attached to a bug report. Serial
// numbers, WWNs and by-id links are redacted. Only dir is written.
func Record(src Source, dir string) error {
	raw, err := src.Lsblk()
	if err != nil {
		return fmt.Errorf("lsblk: %w", err)
	}
	nodes, err := parseLsblk(raw)
	if err != nil {
		return err
	}
	var generic any
	if err := json.Unmarshal(raw, &generic); err != nil {
		return err
	}
	redactJSON(generic)
	lsblk, err := json.MarshalIndent(generic, "", "   ")
	if err != nil {
		return err
	}
	if err := writeFile(dir, "lsblk.json", append(lsblk, '\n')); err != nil {
		return err
	}
	if b, err := src.ReadFile("proc/cmdline"); err == nil {
		if err := writeFile(dir, "root/proc/cmdline", b); err != nil {
			return err
		}
	}
	for _, n := range nodes {
		if n.Type != "disk" || isVirtualName(n.Name) {
			continue
		}
		if props, err := src.Udev(n.devPath()); err == nil && props != nil {
			if err := writeFile(dir, "udev/"+n.Name, formatUdev(redactUdev(props))); err != nil {
				return err
			}
		}
		if strings.HasPrefix(n.Name, "nvme") {
			name := "sys/block/" + n.Name + "/device/model"
			if b, err := src.ReadFile(name); err == nil {
				if err := writeFile(dir, "root/"+name, b); err != nil {
					return err
				}
			}
		}
	}
	return nil
}

const redacted = "REDACTED"

func redactJSON(v any) {
	switch t := v.(type) {
	case map[string]any:
		for k, val := range t {
			if (k == "serial" || k == "wwn") && val != nil {
				t[k] = redacted
				continue
			}
			if str, ok := val.(string); ok && (k == "mountpoint") {
				t[k] = redactPath(str)
				continue
			}
			if k == "mountpoints" {
				if list, ok := val.([]any); ok {
					for i, m := range list {
						if str, ok := m.(string); ok {
							list[i] = redactPath(str)
						}
					}
				}
				continue
			}
			redactJSON(val)
		}
	case []any:
		for _, e := range t {
			redactJSON(e)
		}
	}
}

var userPath = regexp.MustCompile(`^(/run/media|/media|/home)/[^/]+`)

// redactPath hides user names in mountpoints (/run/media/<user>/…).
func redactPath(p string) string { return userPath.ReplaceAllString(p, "$1/user") }

func redactUdev(p map[string]string) map[string]string {
	out := map[string]string{}
	for k, v := range p {
		switch {
		case k == "DEVLINKS":
			continue // by-id links embed the serial number
		case strings.Contains(k, "SERIAL") || strings.HasPrefix(k, "ID_WWN") || k == "ID_SCSI_SERIAL":
			out[k] = redacted
		default:
			out[k] = v
		}
	}
	return out
}

func formatUdev(p map[string]string) []byte {
	keys := make([]string, 0, len(p))
	for k := range p {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	var b strings.Builder
	for _, k := range keys {
		b.WriteString(k + "=" + p[k] + "\n")
	}
	return []byte(b.String())
}

func writeFile(dir, name string, data []byte) error {
	p := filepath.Join(dir, filepath.FromSlash(name))
	if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
		return err
	}
	return os.WriteFile(p, data, 0o644)
}
