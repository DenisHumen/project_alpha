// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"bytes"
	"encoding/json"
	"flag"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

var update = flag.Bool("update", false, "rewrite testdata/*/expected.txt")

// TestGolden replays every recorded hardware configuration in testdata and
// compares the rendered list (all devices, with SMART where recorded).
func TestGolden(t *testing.T) {
	dirs, err := filepath.Glob("testdata/*/lsblk.json")
	if err != nil {
		t.Fatal(err)
	}
	if len(dirs) < 10 {
		t.Fatalf("expected at least 10 hardware configurations, got %d", len(dirs))
	}
	for _, f := range dirs {
		dir := filepath.Dir(f)
		t.Run(filepath.Base(dir), func(t *testing.T) {
			ds, err := Probe(Dir(dir), Options{Smart: true})
			if err != nil {
				t.Fatal(err)
			}
			var buf bytes.Buffer
			if err := RenderText(&buf, ds, true); err != nil {
				t.Fatal(err)
			}
			golden := filepath.Join(dir, "expected.txt")
			if *update {
				if err := os.WriteFile(golden, buf.Bytes(), 0o644); err != nil {
					t.Fatal(err)
				}
				return
			}
			want, err := os.ReadFile(golden)
			if err != nil {
				t.Fatalf("%v (run go test -update)", err)
			}
			if got := buf.String(); got != string(want) {
				t.Errorf("output mismatch (go test -update to accept)\n--- got\n%s\n--- want\n%s", got, want)
			}
			// JSON must round-trip: it is the contract for the TUI and the web installer.
			if _, err := json.Marshal(ds); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func probe(t *testing.T, cfg string) map[string]Disk {
	t.Helper()
	ds, err := Probe(Dir(filepath.Join("testdata", cfg)), Options{Smart: true})
	if err != nil {
		t.Fatal(err)
	}
	m := map[string]Disk{}
	for _, d := range ds {
		m[d.Name] = d
	}
	return m
}

func TestLiveUSBNAS(t *testing.T) {
	m := probe(t, "01-live-usb-nas")
	nv := m["nvme0n1"]
	if nv.Label() != "Samsung 990 PRO 1TB — /dev/nvme0n1" || nv.SizeString() != "1.0 TB" || nv.KindLabel() != "NVMe" {
		t.Errorf("nvme: %q %q %q", nv.Label(), nv.SizeString(), nv.KindLabel())
	}
	if nv.Excluded || nv.HasData() {
		t.Errorf("empty NVMe must be a clean candidate: %+v", nv)
	}
	if m["sda"].Model != "Samsung 870 EVO 1TB" || m["sda"].KindLabel() != "SATA SSD" {
		t.Errorf("sda: %q %q", m["sda"].Model, m["sda"].KindLabel())
	}
	if m["sdb"].Model != "WD WD40EFRX-68N32N0" || m["sdb"].SizeString() != "4.0 TB" || m["sdb"].KindLabel() != "SATA HDD" {
		t.Errorf("sdb: %q %q %q", m["sdb"].Model, m["sdb"].SizeString(), m["sdb"].KindLabel())
	}
	if d := m["sdd"]; d.ExcludeReason != ExcludeBootMedia || d.Model != "SanDisk Ultra" {
		t.Errorf("live stick must be excluded as boot media: %q %q", d.Model, d.ExcludeReason)
	}
	if m["zram0"].ExcludeReason != ExcludeVirtual || m["loop0"].ExcludeReason != ExcludeVirtual {
		t.Error("zram/loop must be excluded")
	}
	if got := len(Candidates(probeList(t, "01-live-usb-nas"))); got != 4 {
		t.Errorf("want 4 candidates, got %d", got)
	}
}

func probeList(t *testing.T, cfg string) []Disk {
	t.Helper()
	ds, err := Probe(Dir(filepath.Join("testdata", cfg)), Options{})
	if err != nil {
		t.Fatal(err)
	}
	return ds
}

func TestMdadmAndBootByLabel(t *testing.T) {
	m := probe(t, "02-mdadm-reinstall")
	if m["sde"].ExcludeReason != ExcludeBootMedia {
		t.Errorf("stick copied to RAM must still be found by CDLABEL: %q", m["sde"].ExcludeReason)
	}
	sda := m["sda"]
	if !hasFinding(sda, FindingRAIDMember, "alpha:media") || !hasFinding(sda, FindingFilesystem, "media") {
		t.Errorf("sda findings: %v", sda.Findings)
	}
	if !hasFinding(m["sdc"], FindingRAIDMember, "old:0") {
		t.Errorf("whole-disk md member not detected: %v", m["sdc"].Findings)
	}
	if !hasFinding(m["sdb"], FindingSmartFailed, "") || m["sdb"].Smart == nil || m["sdb"].Smart.Passed {
		t.Errorf("sdb SMART failure not reported: %+v", m["sdb"])
	}
	if m["nvme0n1"].Model != "Kingston SNV2S1000G" || !hasFinding(m["nvme0n1"], FindingEFISystem, "") {
		t.Errorf("nvme: %q %v", m["nvme0n1"].Model, m["nvme0n1"].Findings)
	}
}

func TestVirtio(t *testing.T) {
	m := probe(t, "03-vm-virtio")
	if d := m["vda"]; d.Model != "Virtual disk 69 GB" || d.Transport != TransportVirtio || d.Media != MediaVirtual {
		t.Errorf("vda: %+v", d)
	}
	if m["sr0"].ExcludeReason != ExcludeOptical {
		t.Errorf("sr0: %q", m["sr0"].ExcludeReason)
	}
}

func TestEMMC(t *testing.T) {
	m := probe(t, "06-n100-emmc")
	if m["mmcblk0boot0"].ExcludeReason != ExcludeHWArea || m["mmcblk0boot1"].ExcludeReason != ExcludeHWArea {
		t.Error("eMMC boot areas must be excluded")
	}
	if d := m["mmcblk0"]; d.Excluded || d.KindLabel() != "eMMC" || d.Model != "eMMC 63 GB" {
		t.Errorf("mmcblk0: %q %q excluded=%v", d.Model, d.KindLabel(), d.Excluded)
	}
	if m["nvme0n1"].Model != "WD Black SN850X 1000GB" {
		t.Errorf("nvme: %q", m["nvme0n1"].Model)
	}
}

func TestUSBMix(t *testing.T) {
	m := probe(t, "07-usb-mix")
	want := map[string]ExcludeReason{"sda": ExcludeBootMedia, "sdc": ExcludeNoMedia, "sdd": ExcludeTooSmall}
	for name, r := range want {
		if m[name].ExcludeReason != r {
			t.Errorf("%s: want %q, got %q", name, r, m[name].ExcludeReason)
		}
	}
	if m["sdb"].Model != "WD Elements 2621" || m["sde"].Model != "JMicron USB drive 500 GB" {
		t.Errorf("names: %q %q", m["sdb"].Model, m["sde"].Model)
	}
}

func TestLegacyLsblk(t *testing.T) {
	m := probe(t, "08-legacy-lsblk-sas")
	if d := m["sda"]; d.Path != "/dev/sda" || !d.Rotational || d.Size != 10000831348736 || d.Model != "HGST HUH721010AL5200" {
		t.Errorf("sda: %+v", d)
	}
	if m["sdb"].Model != "Seagate ST4000NM0035-1V4" {
		t.Errorf("sdb: %q", m["sdb"].Model)
	}
	if !hasFinding(m["sdc"], FindingLVMMember, "") || !hasFinding(m["nvme0n1"], FindingLUKS, "") ||
		!hasFinding(m["sda"], FindingZFSMember, "tank") {
		t.Error("stacked storage not detected")
	}
	if m["nvme0n1"].Model != "Micron 2450 MTFDKBA512TFK" {
		t.Errorf("nvme: %q", m["nvme0n1"].Model)
	}
	if m["sr0"].ExcludeReason != ExcludeOptical {
		t.Errorf("sr0: %q", m["sr0"].ExcludeReason)
	}
}

func TestSysfsModelAndBootByDevice(t *testing.T) {
	m := probe(t, "09-dual-nvme-previous-install")
	if m["nvme1n1"].Model != "Crucial CT1000P3PSSD8" {
		t.Errorf("model from sysfs: %q", m["nvme1n1"].Model)
	}
	if m["sda"].ExcludeReason != ExcludeBootMedia {
		t.Errorf("root=live:/dev/sda1: %q", m["sda"].ExcludeReason)
	}
	if !hasFinding(m["nvme0n1"], FindingFilesystem, "media") {
		t.Errorf("previous data partition not reported: %v", m["nvme0n1"].Findings)
	}
}

func TestRunningSystemExcluded(t *testing.T) {
	m := probe(t, "10-workstation-host")
	if m["nvme0n1"].ExcludeReason != ExcludeBootMedia {
		t.Errorf("disk with / must be excluded: %q", m["nvme0n1"].ExcludeReason)
	}
	if m["nvme0n1"].Model != "Samsung 970 EVO Plus 250GB" || m["sda"].Model != "WD WD10EZEX-08WN4A0" {
		t.Errorf("names: %q %q", m["nvme0n1"].Model, m["sda"].Model)
	}
}

func hasFinding(d Disk, k FindingKind, label string) bool {
	for _, f := range d.Findings {
		if f.Kind == k && (label == "" || f.Label == label) {
			return true
		}
	}
	return false
}

func TestHumanName(t *testing.T) {
	cases := []struct{ model, vendor, want string }{
		{"Samsung SSD 990 PRO 1TB", "", "Samsung 990 PRO 1TB"},
		{"Samsung SSD 870 EVO 1TB  ", "ATA", "Samsung 870 EVO 1TB"},
		{"SAMSUNG MZVL21T0HCLR-00B00", "", "Samsung MZVL21T0HCLR-00B00"},
		{"Samsung SSD", "", "Samsung SSD"}, // nothing left to show after dropping "SSD"
		{"WDC WD40EFRX-68N32N0", "ATA", "WD WD40EFRX-68N32N0"},
		{"WD_BLACK SN850X 1000GB", "", "WD Black SN850X 1000GB"},
		{"WD Blue SN570 1TB", "", "WD Blue SN570 1TB"},
		{"ST4000VN008-2DR166", "ATA", "Seagate ST4000VN008-2DR166"},
		{"CT1000MX500SSD1", "ATA", "Crucial CT1000MX500SSD1"},
		{"KINGSTON SA400S37480G", "ATA", "Kingston SA400S37480G"},
		{"SA400S37480G", "ATA", "Kingston SA400S37480G"},
		{"TOSHIBA MG08ACA16TE", "ATA", "Toshiba MG08ACA16TE"},
		{"INTEL SSDPEKNW010T8", "", "Intel SSDPEKNW010T8"},
		{"SanDisk SSD PLUS 1000GB", "ATA", "SanDisk SSD PLUS 1000GB"},
		{"Micron_2450_MTFDKBA512TFK", "", "Micron 2450 MTFDKBA512TFK"},
		{"HUH721010AL5200", "HGST", "HGST HUH721010AL5200"},
		{"ST4000NM0035-1V4", "SEAGATE", "Seagate ST4000NM0035-1V4"},
		{"Ultra", "SanDisk ", "SanDisk Ultra"},
		{"Elements 2621", "WD", "WD Elements 2621"},
		{"SD/MMC", "Generic-", "SD/MMC"},
		{"Portable SSD T7", "Samsung", "Samsung Portable SSD T7"},
		{"QEMU HARDDISK", "ATA", "QEMU HARDDISK"},
		{"", "0x1af4", ""},
		{"", "", ""},
		{"STORAGE DEVICE", "Generic", "STORAGE DEVICE"},
	}
	for _, c := range cases {
		if got := HumanName(c.model, c.vendor); got != c.want {
			t.Errorf("HumanName(%q, %q) = %q, want %q", c.model, c.vendor, got, c.want)
		}
	}
}

func TestFormatDecimal(t *testing.T) {
	cases := []struct {
		n    uint64
		want string
	}{
		{0, "0 B"},
		{999, "999 B"},
		{4096, "4.1 kB"},
		{15931539456, "16 GB"},
		{31004295168, "31 GB"},
		{250059350016, "250 GB"},
		{500107862016, "500 GB"},
		{512110190592, "512 GB"},
		{999_600_000_000, "1.0 TB"},
		{1000204886016, "1.0 TB"},
		{1024209543168, "1.0 TB"},
		{2000398934016, "2.0 TB"},
		{4000787030016, "4.0 TB"},
		{9_960_000_000_000, "10 TB"},
		{10000831348736, "10 TB"},
		{16000900661248, "16 TB"},
	}
	for _, c := range cases {
		if got := FormatDecimal(c.n); got != c.want {
			t.Errorf("FormatDecimal(%d) = %q, want %q", c.n, got, c.want)
		}
	}
}

func TestParseCmdline(t *testing.T) {
	bi := parseCmdline(`BOOT_IMAGE=/vmlinuz root=live:CDLABEL=Alpha\x20OS rd.live.image`)
	if len(bi.labels) != 1 || bi.labels[0] != "Alpha OS" {
		t.Errorf("labels: %q", bi.labels)
	}
	if bi := parseCmdline("root=UUID=1234 ro"); len(bi.labels)+len(bi.uuids)+len(bi.devs) != 0 {
		t.Errorf("non-live root must give no hints: %+v", bi)
	}
}

func TestNaturalSort(t *testing.T) {
	if !naturalLess("sdz", "sdaa") || !naturalLess("nvme2n1", "nvme10n1") || naturalLess("sdb", "sda") {
		t.Error("natural order broken")
	}
}

func TestParseSmartNoStatus(t *testing.T) {
	h, err := ParseSmartHealth([]byte(`{"smartctl":{"exit_status":4},"device":{"name":"/dev/vda"}}`))
	if err != nil || h.Available {
		t.Errorf("got %+v, %v", h, err)
	}
}

func TestRecordRedacts(t *testing.T) {
	out := t.TempDir()
	if err := Record(Dir("testdata/01-live-usb-nas"), out); err != nil {
		t.Fatal(err)
	}
	b, err := os.ReadFile(filepath.Join(out, "lsblk.json"))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(b), "S73VNJ0W123456X") || !strings.Contains(string(b), redacted) {
		t.Error("serial number not redacted")
	}
	// The recording must replay to the same disks (minus serials).
	a, _ := Probe(Dir("testdata/01-live-usb-nas"), Options{})
	r, err := Probe(Dir(out), Options{})
	if err != nil || len(a) != len(r) {
		t.Fatalf("replay: %v, %d vs %d", err, len(a), len(r))
	}
	for i := range a {
		if a[i].Model != r[i].Model || a[i].ExcludeReason != r[i].ExcludeReason {
			t.Errorf("%s: %q/%q vs %q/%q", a[i].Name, a[i].Model, a[i].ExcludeReason, r[i].Model, r[i].ExcludeReason)
		}
	}
	if got := redactPath("/run/media/someone/SHARED"); got != "/run/media/user/SHARED" {
		t.Errorf("redactPath: %q", got)
	}
}
