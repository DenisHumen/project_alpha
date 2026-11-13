// SPDX-License-Identifier: MPL-2.0

// Command alpha-installer is the NAS OS installer. For now it only has the
// read-only `disks` subcommand (DIST-004); partitioning and the TUI come in
// DIST-005 and DIST-006.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/DenisHumen/project_alpha/distro/installer/internal/disks"
)

func main() {
	if err := run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "alpha-installer:", err)
		os.Exit(1)
	}
}

func run(args []string, stdout io.Writer) error {
	if len(args) == 0 || args[0] != "disks" {
		return fmt.Errorf("usage: alpha-installer disks [--json] [--all] [--smart] [--from DIR] [--record DIR]")
	}
	fs := flag.NewFlagSet("disks", flag.ContinueOnError)
	asJSON := fs.Bool("json", false, "print JSON instead of a table")
	all := fs.Bool("all", false, "also list excluded devices (boot media, loop, zram, optical…)")
	smart := fs.Bool("smart", false, "query SMART health (smartctl -H) for candidate disks")
	from := fs.String("from", "", "read a recorded snapshot directory instead of the live system")
	record := fs.String("record", "", "save a redacted snapshot of this machine to DIR and exit")
	if err := fs.Parse(args[1:]); err != nil {
		return err
	}

	var src disks.Source = disks.System{}
	if *from != "" {
		src = disks.Dir(*from)
	}
	if *record != "" {
		return disks.Record(src, *record)
	}
	list, err := disks.Probe(src, disks.Options{Smart: *smart})
	if err != nil {
		return err
	}
	if *asJSON {
		if !*all {
			list = disks.Candidates(list)
		}
		enc := json.NewEncoder(stdout)
		enc.SetIndent("", "  ")
		return enc.Encode(list)
	}
	return disks.RenderText(stdout, list, *all)
}
