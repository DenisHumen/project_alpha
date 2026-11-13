// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'branding.dart';
import 'fakes/fake_library.dart';
import 'l10n/l10n.dart';
import 'licenses.dart';
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
    this.libmpvMissing = false,
  });

  /// Shows a dismissible banner: libmpv could not be loaded (Linux).
  final bool libmpvMissing;

  final AppServices services;
  final FormFactor formFactor;
  final Section initialSection;

  @override
  State<KinoluneApp> createState() => _KinoluneAppState();
}

class _KinoluneAppState extends State<KinoluneApp> {
  late AppServices _services = widget.services;
  late bool _showLibmpvWarning = widget.libmpvMissing;

  @override
  void initState() {
    super.initState();
    registerAppLicenses();
  }

  Future<void> _useDemo() async {
    final demo = AppServices(
      library: FakeLibrary.demo(),
      secureStore: _services.secureStore,
      preferences: _services.preferences,
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
        child: ValueListenableBuilder<Locale?>(
          valueListenable: _services.locale,
          builder: (context, locale, _) => _app(tv, locale),
        ),
      ),
    );
  }

  Widget _app(bool tv, Locale? locale) => MaterialApp(
    title: kAppName,
    // null = system locale; unsupported system locales fall back to
    // English (the first supported locale).
    locale: locale,
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: kLocalizationsDelegates,
    debugShowCheckedModeBanner: false,
    theme: buildKinoluneTheme(widget.formFactor),
    darkTheme: buildKinoluneTheme(widget.formFactor),
    themeMode: ThemeMode.dark,
    shortcuts: {
      ...WidgetsApp.defaultShortcuts,
      // D-pad centre / remote OK.
      const SingleActivator(LogicalKeyboardKey.select): const ActivateIntent(),
    },
    // On TV, the focus highlight must always be visible.
    builder: (context, child) {
      if (tv) {
        FocusManager.instance.highlightStrategy =
            FocusHighlightStrategy.alwaysTraditional;
      }
      if (!_showLibmpvWarning) return child!;
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
                    Expanded(
                      child: Text(context.l10n.appLibmpvMissing(kAppName)),
                    ),
                    TextButton(
                      onPressed: () =>
                          setState(() => _showLibmpvWarning = false),
                      child: Text(context.l10n.commonDismiss),
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
  );
}
