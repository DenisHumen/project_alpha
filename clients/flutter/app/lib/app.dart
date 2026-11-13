// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'branding.dart';
import 'fakes/fake_library.dart';
import 'platform/form_factor.dart';
import 'screens/onboarding_screen.dart';
import 'services/app_services.dart';
import 'shell/app_shell.dart';
import 'theme/kinolune_theme.dart';

class KinoluneApp extends StatefulWidget {
  const KinoluneApp({
    super.key,
    required this.services,
    required this.formFactor,
    this.initialSection = Section.home,
    this.startupWarning,
  });

  /// Shown once as a banner (e.g. libmpv missing on Linux).
  final String? startupWarning;

  final AppServices services;
  final FormFactor formFactor;
  final Section initialSection;

  @override
  State<KinoluneApp> createState() => _KinoluneAppState();
}

class _KinoluneAppState extends State<KinoluneApp> {
  late AppServices _services = widget.services;
  late String? _warning = widget.startupWarning;

  Future<void> _useDemo() async {
    final demo = AppServices(
      library: FakeLibrary.demo(),
      secureStore: _services.secureStore,
    );
    await demo.init();
    setState(() => _services = demo);
  }

  @override
  Widget build(BuildContext context) {
    final tv = widget.formFactor == FormFactor.tv;
    return FormFactorScope(
      formFactor: widget.formFactor,
      child: Services(
        services: _services,
        child: MaterialApp(
          title: kAppName,
          debugShowCheckedModeBanner: false,
          theme: buildKinoluneTheme(widget.formFactor),
          darkTheme: buildKinoluneTheme(widget.formFactor),
          themeMode: ThemeMode.dark,
          shortcuts: {
            ...WidgetsApp.defaultShortcuts,
            // D-pad centre / remote OK.
            const SingleActivator(LogicalKeyboardKey.select):
                const ActivateIntent(),
          },
          // On TV, the focus highlight must always be visible.
          builder: (context, child) {
            if (tv) {
              FocusManager.instance.highlightStrategy =
                  FocusHighlightStrategy.alwaysTraditional;
            }
            final w = _warning;
            if (w == null) return child!;
            return Column(
              children: [
                Material(
                  color: KColors.bgElevated2,
                  child: SafeArea(
                    bottom: false,
                    child: Padding(
                      padding: const EdgeInsets.fromLTRB(16, 10, 8, 10),
                      child: Row(
                        children: [
                          const Icon(
                            Icons.warning_amber_rounded,
                            color: Colors.amber,
                          ),
                          const SizedBox(width: 12),
                          Expanded(child: Text(w)),
                          TextButton(
                            onPressed: () => setState(() => _warning = null),
                            child: const Text('Dismiss'),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
                Expanded(child: child!),
              ],
            );
          },
          home: ListenableBuilder(
            listenable: _services,
            builder: (context, _) {
              if (!_services.loaded) {
                return const Scaffold(body: SizedBox.shrink());
              }
              if (_services.sources.isEmpty) {
                return OnboardingScreen(onTryDemo: _useDemo);
              }
              return AppShell(
                key: ObjectKey(_services),
                initialSection: widget.initialSection,
              );
            },
          ),
        ),
      ),
    );
  }
}
