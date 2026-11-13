// SPDX-License-Identifier: MPL-2.0

package disks

import (
	"encoding/json"
	"fmt"
)

// SmartHealth is the overall SMART verdict (`smartctl -H`).
type SmartHealth struct {
	// Available is false when the device has no SMART (virtio, many USB bridges).
	Available bool `json:"available"`
	Passed    bool `json:"passed"`
}

// ParseSmartHealth parses `smartctl -H -j` output.
func ParseSmartHealth(data []byte) (*SmartHealth, error) {
	var out struct {
		SmartStatus *struct {
			Passed bool `json:"passed"`
		} `json:"smart_status"`
	}
	if err := json.Unmarshal(data, &out); err != nil {
		return nil, fmt.Errorf("parse smartctl json: %w", err)
	}
	if out.SmartStatus == nil {
		return &SmartHealth{}, nil
	}
	return &SmartHealth{Available: true, Passed: out.SmartStatus.Passed}, nil
}

func smartHealth(src Source, devPath string) (*SmartHealth, error) {
	b, err := src.Smart(devPath)
	if err != nil {
		return nil, err
	}
	return ParseSmartHealth(b)
}
