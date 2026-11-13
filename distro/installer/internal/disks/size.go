// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"math"
	"strconv"
)

var decimalUnits = []string{"B", "kB", "MB", "GB", "TB", "PB", "EB"}

// FormatDecimal formats a byte count in decimal (SI) units, the way drive
// boxes do: 1 TB = 10^12 bytes. Values below 10 get one decimal place,
// larger values are rounded to an integer: 1000204886016 → "1.0 TB",
// 250059350016 → "250 GB", 16000900661248 → "16 TB".
func FormatDecimal(n uint64) string {
	if n < 1000 {
		return strconv.FormatUint(n, 10) + " B"
	}
	v := float64(n)
	i := 0
	for v >= 1000 && i < len(decimalUnits)-1 {
		v /= 1000
		i++
	}
	if v < 10 {
		r := math.Round(v*10) / 10
		if r >= 10 {
			return "10 " + decimalUnits[i]
		}
		return strconv.FormatFloat(r, 'f', 1, 64) + " " + decimalUnits[i]
	}
	r := math.Round(v)
	if r >= 1000 && i < len(decimalUnits)-1 {
		// 999.6 GB rounds up to the next unit: "1.0 TB", not "1000 GB".
		return "1.0 " + decimalUnits[i+1]
	}
	return strconv.FormatFloat(r, 'f', 0, 64) + " " + decimalUnits[i]
}
