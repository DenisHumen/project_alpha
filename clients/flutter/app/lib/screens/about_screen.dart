// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../branding.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/focusable.dart';
import '../widgets/logo.dart';

class AboutScreen extends StatelessWidget {
  const AboutScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    return Scaffold(
      body: SafeArea(
        child: ListView(
          padding: EdgeInsets.fromLTRB(
            d.pageInsets.left,
            d.pageInsets.top,
            d.pageInsets.right,
            d.pageInsets.bottom + 90,
          ),
          children: [
            if (!d.isTv)
              Align(
                alignment: Alignment.centerLeft,
                child: IconButton(
                  tooltip: 'Back',
                  onPressed: () => Navigator.of(context).maybePop(),
                  icon: const Icon(Icons.arrow_back),
                ),
              ),
            const SizedBox(height: 8),
            Align(
              alignment: Alignment.centerLeft,
              child: KinoluneLogo(size: d.isTv ? 44 : 56),
            ),
            const SizedBox(height: 16),
            Text('Version $kAppVersion', style: theme.textTheme.labelMedium),
            const SizedBox(height: 12),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 640),
              child: Text(
                '$kAppName is a free, open-source media player for your own library: movies, shows and music '
                'from local folders and network shares. Everything runs on your devices — no accounts, no cloud, '
                'no tracking. Licensed under the Mozilla Public License 2.0.',
                style: theme.textTheme.bodyLarge?.copyWith(
                  color: KColors.textSecondary,
                ),
              ),
            ),
            const SizedBox(height: 8),
            SelectableText(
              kRepoUrl,
              style: theme.textTheme.bodySmall?.copyWith(
                color: KColors.accentSoft,
              ),
            ),
            const SizedBox(height: 24),
            Align(
              alignment: Alignment.centerLeft,
              child: KButton(
                autofocus: !d.isPhone,
                label: 'Open-source licences',
                icon: Icons.description_outlined,
                onPressed: () => showLicensePage(
                  context: context,
                  applicationName: kAppName,
                  applicationVersion: kAppVersion,
                  applicationIcon: const Padding(
                    padding: EdgeInsets.all(12),
                    child: KinoluneLogo(size: 48, showName: false),
                  ),
                  applicationLegalese:
                      'Mozilla Public License 2.0. $kTmdbNotice',
                ),
              ),
            ),
            const SizedBox(height: 32),
            const TmdbAttribution(),
          ],
        ),
      ),
    );
  }
}

/// TMDB attribution required by the API terms. The official TMDB logo is a
/// third-party brand asset: it is added only via design/LICENSES.md (DSN-007),
/// until then a neutral text mark is shown.
class TmdbAttribution extends StatelessWidget {
  const TmdbAttribution({super.key});

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 640),
      child: Container(
        padding: const EdgeInsets.all(16),
        decoration: BoxDecoration(
          color: KColors.bgElevated,
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: KColors.stroke),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
              decoration: BoxDecoration(
                border: Border.all(color: Colors.white54),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Text(
                'TMDB',
                style: theme.textTheme.titleSmall?.copyWith(letterSpacing: 1.5),
              ),
            ),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Metadata and images',
                    style: theme.textTheme.titleMedium,
                  ),
                  const SizedBox(height: 4),
                  Text(kTmdbNotice, style: theme.textTheme.bodySmall),
                  const SizedBox(height: 4),
                  SelectableText(
                    kTmdbUrl,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: KColors.accentSoft,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
