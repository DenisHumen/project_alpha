// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"strconv"
	"strings"
)

// brandCase maps upper-cased first tokens to the brand spelling used on the box.
var brandCase = map[string]string{
	"SAMSUNG":   "Samsung",
	"KINGSTON":  "Kingston",
	"TOSHIBA":   "Toshiba",
	"INTEL":     "Intel",
	"SEAGATE":   "Seagate",
	"HITACHI":   "Hitachi",
	"HGST":      "HGST",
	"SANDISK":   "SanDisk",
	"CRUCIAL":   "Crucial",
	"MICRON":    "Micron",
	"WDC":       "WD",
	"WD":        "WD",
	"WD_BLACK":  "WD Black",
	"WD_BLUE":   "WD Blue",
	"WD_RED":    "WD Red",
	"SK":        "SK",
	"HYNIX":     "Hynix",
	"ADATA":     "ADATA",
	"TRANSCEND": "Transcend",
	"KIOXIA":    "Kioxia",
	"LEXAR":     "Lexar",
	"PATRIOT":   "Patriot",
	"TEAM":      "Team",
	"CORSAIR":   "Corsair",
	"SABRENT":   "Sabrent",
	"VERBATIM":  "Verbatim",
	"PNY":       "PNY",
	"PHISON":    "Phison",
	"QEMU":      "QEMU",
	"VBOX":      "VBOX",
	"VMWARE":    "VMware",
	"VMWARE,":   "VMware",
	"MSFT":      "Msft",
}

// genericVendors are VENDOR values that say nothing about the product
// (SATA reports "ATA", USB bridges report "Generic" and the like).
var genericVendors = map[string]bool{
	"": true, "ATA": true, "NVME": true, "GENERIC": true, "GENERIC-": true, "USB": true,
	"MASS": true, "STORAGE": true, "LINUX": true, "0X1AF4": true, "VIRTIO": true,
}

// modelPrefixes infer the brand for models that are reported without it.
var modelPrefixes = []struct{ prefix, brand string }{
	{"ST", "Seagate"},     // ST4000VN008-2DR166
	{"CT", "Crucial"},     // CT1000MX500SSD1, CT1000P3PSSD8
	{"WD", "WD"},          // WD40EFRX-68N32N0 on SAS/USB, WDS500G2B0A
	{"HUH", "HGST"},       // HUH721010AL5200
	{"HUS", "HGST"},       // HUS726T4TALA6L4
	{"MZ", "Samsung"},     // MZ7LH960HAJR (OEM)
	{"SSDSC", "Intel"},    // SSDSC2KB960G8
	{"SSDPE", "Intel"},    // SSDPEKNW010T8
	{"MTFD", "Micron"},    // MTFDDAK512TBN
	{"SA400", "Kingston"}, // SA400S37480G
	{"SNV", "Kingston"},   // SNV2S1000G
}

// HumanName turns a raw MODEL/VENDOR pair into the name shown to the user,
// e.g. "Samsung SSD 990 PRO 1TB" → "Samsung 990 PRO 1TB",
// "WDC WD40EFRX-68N32N0" → "WD WD40EFRX-68N32N0", ("SanDisk", "Ultra") → "SanDisk Ultra".
// Rules are deliberately conservative: only cosmetic changes that cannot make
// two different products look identical. Returns "" when nothing is known.
func HumanName(model, vendor string) string {
	m := collapseSpaces(model)
	v := collapseSpaces(vendor)
	tokens := strings.Fields(m)

	// "Micron_2450_MTFDKBA512TFK": underscore-joined model with a known brand.
	if len(tokens) == 1 && strings.Contains(tokens[0], "_") {
		first := strings.ToUpper(strings.SplitN(tokens[0], "_", 2)[0])
		if _, ok := brandCase[first]; ok && first != "WD" {
			tokens = strings.Fields(strings.ReplaceAll(tokens[0], "_", " "))
		}
	}

	if len(tokens) > 0 {
		if b, ok := brandCase[strings.ToUpper(tokens[0])]; ok {
			tokens[0] = b
		} else if brand := vendorBrand(v, tokens); brand != "" {
			tokens = append(strings.Fields(brand), tokens...)
		} else if brand := inferBrand(tokens[0]); brand != "" {
			tokens = append([]string{brand}, tokens...)
		}
	} else {
		// No model: the caller builds "<vendor> <kind> <size>" (see fallbackName).
		return ""
	}

	// Samsung puts "SSD" between brand and product line: "Samsung SSD 870 EVO 1TB".
	// The box says "870 EVO", so drop it. Other brands keep the token, because
	// for them it is part of the product name ("SanDisk SSD PLUS").
	if len(tokens) > 2 && tokens[0] == "Samsung" && tokens[1] == "SSD" {
		tokens = append(tokens[:1], tokens[2:]...)
	}
	return strings.Join(tokens, " ")
}

// vendorBrand returns the VENDOR to prepend (USB/SAS/SCSI devices report the
// brand separately), or "" if it is generic or already part of the model.
func vendorBrand(vendor string, tokens []string) string {
	if genericVendors[strings.ToUpper(vendor)] || strings.HasPrefix(strings.ToUpper(vendor), "0X") {
		return ""
	}
	if strings.EqualFold(vendor, "ATA") {
		return ""
	}
	if strings.HasPrefix(strings.ToUpper(strings.Join(tokens, " ")), strings.ToUpper(vendor)) {
		return ""
	}
	return brandOrSelf(vendor)
}

func brandOrSelf(s string) string {
	if b, ok := brandCase[strings.ToUpper(s)]; ok {
		return b
	}
	return s
}

func inferBrand(token string) string {
	up := strings.ToUpper(token)
	for _, p := range modelPrefixes {
		if !strings.HasPrefix(up, p.prefix) || len(up) <= len(p.prefix) {
			continue
		}
		next := up[len(p.prefix)]
		// "ST4000…" but not "STORAGE"; "WDS500…" and "WD40…" but not "WDC".
		if p.prefix == "ST" || p.prefix == "CT" {
			if next < '0' || next > '9' {
				continue
			}
		}
		return p.brand
	}
	return ""
}

// collapseSpaces trims and collapses runs of whitespace (sysfs pads NVMe models).
func collapseSpaces(s string) string {
	return strings.Join(strings.Fields(s), " ")
}

// decodeUdevEnc decodes udev *_ENC values ("Samsung\x20SSD\x20970") .
func decodeUdevEnc(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); i++ {
		if s[i] == '\\' && i+3 < len(s) && s[i+1] == 'x' {
			if v, err := strconv.ParseUint(s[i+2:i+4], 16, 8); err == nil {
				b.WriteByte(byte(v))
				i += 3
				continue
			}
		}
		b.WriteByte(s[i])
	}
	return b.String()
}
