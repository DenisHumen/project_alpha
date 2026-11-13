// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// Source provides raw, read-only system data. System reads the live machine;
// Dir replays a recorded snapshot (tests, bug reports).
type Source interface {
	// Lsblk returns the output of `lsblk` with LsblkArgs.
	Lsblk() ([]byte, error)
	// Udev returns `udevadm info --query=property` for a device; nil if unknown.
	Udev(devPath string) (map[string]string, error)
	// ReadFile reads a file relative to the filesystem root ("sys/block/sda/device/model").
	ReadFile(name string) ([]byte, error)
	// Smart returns `smartctl -H -j` output for a device.
	Smart(devPath string) ([]byte, error)
}

// System is the live Source. Every command it runs is read-only.
type System struct {
	// Timeout per external command; zero means 10 s.
	Timeout time.Duration
}

func (s System) run(name string, args ...string) ([]byte, error) {
	t := s.Timeout
	if t == 0 {
		t = 10 * time.Second
	}
	ctx, cancel := context.WithTimeout(context.Background(), t)
	defer cancel()
	cmd := exec.CommandContext(ctx, name, args...)
	cmd.Env = append(os.Environ(), "LC_ALL=C")
	return cmd.Output()
}

func (s System) Lsblk() ([]byte, error) { return s.run("lsblk", LsblkArgs...) }

func (s System) Udev(devPath string) (map[string]string, error) {
	out, err := s.run("udevadm", "info", "--query=property", "--name="+devPath)
	if err != nil {
		return nil, err
	}
	return parseUdevProperties(out), nil
}

func (s System) ReadFile(name string) ([]byte, error) {
	return os.ReadFile(filepath.Join("/", filepath.Clean("/"+name)))
}

func (s System) Smart(devPath string) ([]byte, error) {
	out, err := s.run("smartctl", "-H", "-j", devPath)
	// smartctl encodes health in its exit status bits; the JSON is still valid.
	var exitErr *exec.ExitError
	if err != nil && errors.As(err, &exitErr) && len(out) > 0 {
		return out, nil
	}
	return out, err
}

// Dir replays a snapshot recorded by `alpha-installer disks --record DIR` (or
// written by hand for tests):
//
//	DIR/lsblk.json            lsblk -J -b -o <LsblkColumns>
//	DIR/udev/<name>           udevadm info --query=property --name=/dev/<name>
//	DIR/smart/<name>.json     smartctl -H -j /dev/<name>
//	DIR/root/proc/cmdline     kernel command line
//	DIR/root/sys/block/...    sysfs files (e.g. sys/block/nvme0n1/device/model)
type Dir string

func (d Dir) Lsblk() ([]byte, error) { return os.ReadFile(filepath.Join(string(d), "lsblk.json")) }

func (d Dir) Udev(devPath string) (map[string]string, error) {
	b, err := os.ReadFile(filepath.Join(string(d), "udev", filepath.Base(devPath)))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return parseUdevProperties(b), nil
}

func (d Dir) ReadFile(name string) ([]byte, error) {
	return os.ReadFile(filepath.Join(string(d), "root", filepath.Clean("/"+name)))
}

func (d Dir) Smart(devPath string) ([]byte, error) {
	return os.ReadFile(filepath.Join(string(d), "smart", filepath.Base(devPath)+".json"))
}

func parseUdevProperties(b []byte) map[string]string {
	props := map[string]string{}
	sc := bufio.NewScanner(bytes.NewReader(b))
	for sc.Scan() {
		k, v, ok := strings.Cut(sc.Text(), "=")
		if ok && k != "" {
			props[k] = v
		}
	}
	return props
}
