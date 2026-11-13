// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"fmt"
	"io"
	"strings"
	"unicode/utf8"
)

// RenderText writes the plain-text disk list used by `alpha-installer disks`
// and the golden tests. With all=false excluded disks are omitted.
//
//	! Samsung 990 PRO 1TB   1.0 TB  NVMe      /dev/nvme0n1  S/N …1234
//	      GPT partition table, 2 partition(s)
func RenderText(w io.Writer, ds []Disk, all bool) error {
	var shown []Disk
	for _, d := range ds {
		if !d.Excluded || all {
			shown = append(shown, d)
		}
	}
	rows := make([][]string, len(shown))
	widths := make([]int, 4)
	for i, d := range shown {
		rows[i] = []string{d.Model, d.SizeString(), d.KindLabel(), d.Path}
		for c, cell := range rows[i] {
			widths[c] = max(widths[c], utf8.RuneCountInString(cell))
		}
	}
	var b strings.Builder
	for i, d := range shown {
		mark := " "
		if d.Excluded {
			mark = "-"
		} else if d.HasData() {
			mark = "!"
		}
		line := mark + " " + pad(rows[i][0], widths[0]) + "  " + padLeft(rows[i][1], widths[1]) +
			"  " + pad(rows[i][2], widths[2]) + "  " + pad(rows[i][3], widths[3])
		if s := d.MaskedSerial(); s != "" {
			line += "  S/N " + s
		}
		b.WriteString(strings.TrimRight(line, " ") + "\n")
		if d.Excluded {
			fmt.Fprintf(&b, "      excluded: %s\n", d.ExcludeReason)
		}
		for _, f := range d.Findings {
			fmt.Fprintf(&b, "      %s\n", f)
		}
		if d.Smart != nil {
			state := "not available"
			if d.Smart.Available && d.Smart.Passed {
				state = "PASSED"
			} else if d.Smart.Available {
				state = "FAILED"
			}
			fmt.Fprintf(&b, "      SMART: %s\n", state)
		}
	}
	_, err := io.WriteString(w, b.String())
	return err
}

func pad(s string, w int) string {
	return s + strings.Repeat(" ", max(0, w-utf8.RuneCountInString(s)))
}

func padLeft(s string, w int) string {
	return strings.Repeat(" ", max(0, w-utf8.RuneCountInString(s))) + s
}
