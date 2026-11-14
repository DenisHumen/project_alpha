// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../branding.dart';
import '../l10n/l10n.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';
import '../widgets/focusable.dart';
import '../widgets/logo.dart';
import '../sources/source_manager.dart';
import 'add_source_dialog.dart';

/// First run: shown while there are no sources.
class OnboardingScreen extends StatelessWidget {
  const OnboardingScreen({super.key, required this.onTryDemo});

  /// Loads the built-in demo library (fictional titles) to look around.
  final VoidCallback onTryDemo;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final l = context.l10n;
    return Scaffold(
      body: Stack(
        fit: StackFit.expand,
        children: [
          const BrandPlaceholder(style: ArtStyle.backdrop),
          const DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.centerLeft,
                end: Alignment.centerRight,
                colors: [Color(0xF20B0B0F), Color(0x800B0B0F)],
              ),
            ),
          ),
          SafeArea(
            child: Center(
              child: SingleChildScrollView(
                padding: EdgeInsets.symmetric(
                  horizontal: d.pageInsets.left,
                  vertical: 24,
                ),
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 620),
                  child: FocusTraversalGroup(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        KinoluneLogo(size: d.isTv ? 48 : 56),
                        SizedBox(height: d.isTv ? 20 : 32),
                        Text(
                          l.onboardingHeadline,
                          style: theme.textTheme.displayMedium,
                        ),
                        const SizedBox(height: 14),
                        Text(
                          l.onboardingBody(kAppName),
                          style: theme.textTheme.bodyLarge?.copyWith(
                            color: KColors.textSecondary,
                          ),
                        ),
                        SizedBox(height: d.isTv ? 24 : 32),
                        Wrap(
                          spacing: 12,
                          runSpacing: 12,
                          children: [
                            KButton(
                              autofocus: !d.isPhone,
                              primary: true,
                              icon: Icons.create_new_folder_outlined,
                              label: l.onboardingAddFolder,
                              onPressed: () =>
                                  showAddSourceDialog(context, type: SourceType.local),
                            ),
                            KButton(
                              icon: Icons.auto_awesome_outlined,
                              label: l.onboardingTryDemo,
                              onPressed: onTryDemo,
                            ),
                          ],
                        ),
                        const SizedBox(height: 18),
                        Text(
                          l.onboardingSmbNote,
                          style: theme.textTheme.bodySmall,
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
