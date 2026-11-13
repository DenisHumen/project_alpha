// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../branding.dart';
import '../l10n/l10n.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/focusable.dart';
import '../widgets/logo.dart';

class AboutScreen extends StatelessWidget {
  const AboutScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final theme = Theme.of(context);
    final l = context.l10n;
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
                  tooltip: l.commonBack,
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
            Text(
              l.aboutVersion(kAppVersion),
              style: theme.textTheme.labelMedium,
            ),
            const SizedBox(height: 12),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 640),
              child: Text(
                l.aboutDescription(kAppName),
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
                label: l.aboutLicenses,
                icon: Icons.description_outlined,
                onPressed: () => showLicensePage(
                  context: context,
                  applicationName: kAppName,
                  applicationVersion: kAppVersion,
                  applicationIcon: const Padding(
                    padding: EdgeInsets.all(12),
                    child: KinoluneLogo(size: 48, showName: false),
                  ),
                  // Legal notice, kept verbatim in English.
                  applicationLegalese:
                      'Mozilla Public License 2.0. $kTmdbNotice', // l10n-ignore
                ),
              ),
            ),
            const SizedBox(height: 32),
            const TmdbAttribution(),
            const SizedBox(height: 16),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 640),
              child: Text(
                l.aboutThirdParty,
                style: theme.textTheme.bodySmall,
              ),
            ),
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
    final l = context.l10n;
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
                'TMDB', // l10n-ignore: brand name
                style: theme.textTheme.titleSmall?.copyWith(letterSpacing: 1.5),
              ),
            ),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    l.aboutTmdbHeading,
                    style: theme.textTheme.titleMedium,
                  ),
                  const SizedBox(height: 4),
                  Text(l.commonTmdbNotice, style: theme.textTheme.bodySmall),
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
