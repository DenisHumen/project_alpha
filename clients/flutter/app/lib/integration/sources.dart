// SPDX-License-Identifier: MPL-2.0
//
// Single import point for the Rust source manager (`pa-sources`, NET-011)
// from pa_core. Only lib/sources/core_source_manager.dart uses it.

export 'package:pa_core/pa_core.dart'
    show
        CoreManagedSource,
        CoreSourceInput,
        CoreSourceOptions,
        CoreSourceStatus,
        CoreSources,
        CoreSpeedReport,
        SourcesError;
