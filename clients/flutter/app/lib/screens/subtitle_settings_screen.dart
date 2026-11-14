// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/player.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/focusable.dart';

/// Subtitle style (PLAY-007): the system caption style by default, or the
/// user's size, font, text color and background; the position always.
/// A preview shows the result as the player draws it.
class SubtitleSettingsScreen extends StatefulWidget {
  const SubtitleSettingsScreen({super.key, this.captionStyleLoader});

  /// Reads the system caption style; default [SystemCaptionStyle.read].
  final Future<SystemCaptionStyle?> Function()? captionStyleLoader;

  @override
  State<SubtitleSettingsScreen> createState() => _SubtitleSettingsScreenState();
}

class _SubtitleSettingsScreenState extends State<SubtitleSettingsScreen>
    with WidgetsBindingObserver {
  SystemCaptionStyle? _system;
  bool _systemRead = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _readSystem();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // Back from the system caption settings.
    if (state == AppLifecycleState.resumed) _readSystem();
  }

  Future<void> _readSystem() async {
    final load = widget.captionStyleLoader ?? SystemCaptionStyle.read;
    SystemCaptionStyle? style;
    try {
      style = await load();
    } catch (_) {
      style = null;
    }
    if (mounted) {
      setState(() {
        _system = style;
        _systemRead = true;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final services = Services.of(context);
    final d = KDensity.of(context);
    final l = context.l10n;
    return Scaffold(
      body: SafeArea(
        child: ValueListenableBuilder<SubtitleSettings>(
          valueListenable: services.subtitleSettings,
          builder: (context, s, _) {
            void set(SubtitleSettings v) => services.setSubtitleSettings(v);
            return ListView(
              padding: EdgeInsets.fromLTRB(
                d.pageInsets.left,
                d.pageInsets.top,
                d.pageInsets.right,
                d.pageInsets.bottom + 90,
              ),
              children: [
                Row(
                  children: [
                    if (!d.isTv)
                      IconButton(
                        tooltip: l.commonBack,
                        onPressed: () => Navigator.of(context).maybePop(),
                        icon: const Icon(Icons.arrow_back),
                      ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        l.subtitlesStyleTitle,
                        style: Theme.of(context).textTheme.headlineSmall,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 760),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      _Preview(
                        appearance: s.resolve(_system),
                        maxWidth: d.isTv ? 320 : 480,
                      ),
                      const SizedBox(height: 16),
                      _SwitchTile(
                        key: const ValueKey('subtitles-system'),
                        title: l.subtitlesSystemStyle,
                        subtitle: !_systemRead
                            ? null
                            : _system == null
                            ? l.subtitlesSystemStyleNone
                            : _system!.enabled
                            ? l.subtitlesSystemStyleOn
                            : l.subtitlesSystemStyleDisabled,
                        value: s.useSystemStyle,
                        onChanged: (v) => set(s.copyWith(useSystemStyle: v)),
                      ),
                      if (!s.useSystemStyle) ...[
                        _Label(l.subtitlesSize),
                        _Choices<double>(
                          keyPrefix: 'subtitles-size',
                          values: SubtitleSettings.scales,
                          selected: s.scale,
                          label: (v) => l.subtitlesPercent((v * 100).round()),
                          onSelected: (v) => set(s.copyWith(scale: v)),
                        ),
                        _Label(l.subtitlesFont),
                        _Choices<SubtitleFont>(
                          keyPrefix: 'subtitles-font',
                          values: SubtitleFont.values,
                          selected: s.font,
                          label: (f) => _fontName(l, f),
                          onSelected: (v) => set(s.copyWith(font: v)),
                        ),
                        _Label(l.subtitlesTextColor),
                        _Choices<Color>(
                          keyPrefix: 'subtitles-color',
                          values: SubtitleSettings.textColors,
                          selected: s.textColor,
                          label: (c) => _colorName(l, c),
                          swatch: true,
                          onSelected: (v) => set(s.copyWith(textColor: v)),
                        ),
                        _Label(l.subtitlesBackground),
                        _Choices<SubtitleBackground>(
                          keyPrefix: 'subtitles-background',
                          values: SubtitleBackground.values,
                          selected: s.background,
                          label: (b) => switch (b) {
                            SubtitleBackground.none =>
                              l.subtitlesBackgroundNone,
                            SubtitleBackground.translucent =>
                              l.subtitlesBackgroundTranslucent,
                            SubtitleBackground.opaque =>
                              l.subtitlesBackgroundOpaque,
                          },
                          onSelected: (v) => set(s.copyWith(background: v)),
                        ),
                      ],
                      _Label(l.subtitlesPosition),
                      Row(
                        children: [
                          IconButton(
                            key: const ValueKey('subtitles-position-down'),
                            tooltip: l.subtitlesPositionDown,
                            onPressed:
                                s.position <= SubtitleSettings.minPosition
                                ? null
                                : () => set(
                                    s.copyWith(
                                      position:
                                          s.position -
                                          SubtitleSettings.positionStep,
                                    ),
                                  ),
                            icon: const Icon(Icons.arrow_downward),
                          ),
                          Expanded(
                            child: Text(
                              l.subtitlesPositionValue(
                                (s.position * 100).round(),
                              ),
                              key: const ValueKey('subtitles-position'),
                              textAlign: TextAlign.center,
                            ),
                          ),
                          IconButton(
                            key: const ValueKey('subtitles-position-up'),
                            tooltip: l.subtitlesPositionUp,
                            onPressed:
                                s.position >= SubtitleSettings.maxPosition
                                ? null
                                : () => set(
                                    s.copyWith(
                                      position:
                                          s.position +
                                          SubtitleSettings.positionStep,
                                    ),
                                  ),
                            icon: const Icon(Icons.arrow_upward),
                          ),
                        ],
                      ),
                      const SizedBox(height: 16),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: KButton(
                          label: l.subtitlesReset,
                          icon: Icons.restart_alt,
                          onPressed: s == const SubtitleSettings()
                              ? null
                              : () => set(const SubtitleSettings()),
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }

  static String _fontName(AppLocalizations l, SubtitleFont f) => switch (f) {
    SubtitleFont.sansSerif => l.subtitlesFontSans,
    SubtitleFont.serif => l.subtitlesFontSerif,
    SubtitleFont.monospace => l.subtitlesFontMono,
    SubtitleFont.casual => l.subtitlesFontCasual,
    SubtitleFont.cursive => l.subtitlesFontCursive,
    SubtitleFont.condensed => l.subtitlesFontCondensed,
    SubtitleFont.smallCaps => l.subtitlesFontSmallCaps,
  };

  static String _colorName(AppLocalizations l, Color c) =>
      switch (c.toARGB32()) {
        0xFFFFFFFF => l.subtitlesColorWhite,
        0xFFFFFF00 => l.subtitlesColorYellow,
        0xFF00FFFF => l.subtitlesColorCyan,
        0xFF00FF00 => l.subtitlesColorGreen,
        0xFFFF00FF => l.subtitlesColorMagenta,
        _ => l.subtitlesColorBlack,
      };
}

/// A 16:9 frame with sample subtitles in the chosen look.
class _Preview extends StatelessWidget {
  const _Preview({required this.appearance, required this.maxWidth});
  final SubtitleAppearance appearance;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return ConstrainedBox(
      constraints: BoxConstraints(maxWidth: maxWidth),
      child: AspectRatio(
        aspectRatio: 16 / 9,
        child: ClipRRect(
          borderRadius: BorderRadius.circular(12),
          child: DecoratedBox(
            key: const ValueKey('subtitles-preview'),
            decoration: const BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                colors: [
                  Color(0xFF3A4F6B),
                  Color(0xFF8A6F4E),
                  Color(0xFF1B1F27),
                ],
              ),
            ),
            child: SubtitleOverlay(
              lines: [l.subtitlesPreviewLine1, l.subtitlesPreviewLine2],
              appearance: appearance,
            ),
          ),
        ),
      ),
    );
  }
}

class _Label extends StatelessWidget {
  const _Label(this.text);
  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 18, bottom: 8),
    child: Text(text, style: Theme.of(context).textTheme.titleMedium),
  );
}

class _SwitchTile extends StatelessWidget {
  const _SwitchTile({
    super.key,
    required this.title,
    required this.value,
    required this.onChanged,
    this.subtitle,
  });

  final String title;
  final String? subtitle;
  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Semantics(
      toggled: value,
      child: KFocusable(
        onActivate: () => onChanged(!value),
        focusScale: 1.0,
        light: false,
        radius: 12,
        semanticLabel: title,
        builder: (context, focused) => AnimatedContainer(
          duration: KMotion.focus,
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          color: focused
              ? Colors.white.withValues(alpha: .09)
              : Colors.transparent,
          child: Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title, style: theme.textTheme.titleMedium),
                    if (subtitle != null)
                      Text(subtitle!, style: theme.textTheme.labelSmall),
                  ],
                ),
              ),
              ExcludeFocus(
                child: ExcludeSemantics(
                  child: Switch(value: value, onChanged: onChanged),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// A wrap of choice chips (focusable with a D-pad).
class _Choices<T> extends StatelessWidget {
  const _Choices({
    required this.keyPrefix,
    required this.values,
    required this.selected,
    required this.label,
    required this.onSelected,
    this.swatch = false,
  });

  final String keyPrefix;
  final List<T> values;
  final T selected;
  final String Function(T) label;
  final ValueChanged<T> onSelected;

  /// Values are colors: show a swatch next to the name.
  final bool swatch;

  @override
  Widget build(BuildContext context) => FocusTraversalGroup(
    child: Wrap(
      spacing: 8,
      runSpacing: 8,
      children: [
        for (var i = 0; i < values.length; i++)
          ChoiceChip(
            key: ValueKey('$keyPrefix-$i'),
            label: Text(label(values[i])),
            avatar: swatch
                ? CircleAvatar(
                    backgroundColor: values[i] as Color,
                    child: const SizedBox.shrink(),
                  )
                : null,
            selected: values[i] == selected,
            onSelected: (_) => onSelected(values[i]),
          ),
      ],
    ),
  );
}
