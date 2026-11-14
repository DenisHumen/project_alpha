// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../l10n/l10n.dart';
import '../widgets/cards.dart';

/// Placeholder of the Twitch section (UI-002). The section is hidden by
/// default; the Twitch workstream replaces this screen.
class TwitchScreen extends StatelessWidget {
  const TwitchScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        PageTitle(l.navTwitch),
        Expanded(
          child: EmptyState(
            icon: Icons.live_tv_outlined,
            title: l.twitchComingTitle,
            message: l.twitchComingMessage,
          ),
        ),
      ],
    );
  }
}
