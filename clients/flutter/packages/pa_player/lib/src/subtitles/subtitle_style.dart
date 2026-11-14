// SPDX-License-Identifier: MPL-2.0
import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Font family of subtitles. The names follow the Android captioning
/// typefaces; other platforms map them to their generic families.
enum SubtitleFont {
  sansSerif,
  serif,
  monospace,
  casual,
  cursive,
  condensed,
  smallCaps,
}

/// Edge around the glyphs (Android `CaptionStyle.EDGE_TYPE_*`).
enum SubtitleEdge { none, outline, dropShadow, raised, depressed }

/// Background behind the text, as offered in the settings.
enum SubtitleBackground {
  /// No box: outlined text.
  none,

  /// Semi-transparent black box.
  translucent,

  /// Solid black box.
  opaque,
}

/// How subtitles look on screen: the resolved [SubtitleSettings], with the
/// system caption style applied when it is used.
@immutable
class SubtitleAppearance {
  const SubtitleAppearance({
    this.fontScale = 1.0,
    this.font = SubtitleFont.sansSerif,
    this.textColor = const Color(0xFFFFFFFF),
    this.backgroundColor = const Color(0x00000000),
    this.windowColor = const Color(0x00000000),
    this.edge = SubtitleEdge.outline,
    this.edgeColor = const Color(0xFF000000),
    this.bottomMargin = SubtitleSettings.defaultPosition,
  });

  /// Size relative to the default ([textSizeFraction] of the video height).
  final double fontScale;
  final SubtitleFont font;
  final Color textColor;

  /// Box behind each line.
  final Color backgroundColor;

  /// Box behind the whole cue.
  final Color windowColor;
  final SubtitleEdge edge;
  final Color edgeColor;

  /// Distance of the last line from the bottom, as a fraction of the height.
  final double bottomMargin;

  /// Text height at scale 1 as a fraction of the picture height (the
  /// default of Android's caption renderers).
  static const double textSizeFraction = 0.0533;

  /// The default look: white outlined sans-serif text near the bottom.
  static const standard = SubtitleAppearance();

  SubtitleAppearance copyWith({double? bottomMargin}) => SubtitleAppearance(
    fontScale: fontScale,
    font: font,
    textColor: textColor,
    backgroundColor: backgroundColor,
    windowColor: windowColor,
    edge: edge,
    edgeColor: edgeColor,
    bottomMargin: bottomMargin ?? this.bottomMargin,
  );

  @override
  bool operator ==(Object other) =>
      other is SubtitleAppearance &&
      other.fontScale == fontScale &&
      other.font == font &&
      other.textColor == textColor &&
      other.backgroundColor == backgroundColor &&
      other.windowColor == windowColor &&
      other.edge == edge &&
      other.edgeColor == edgeColor &&
      other.bottomMargin == bottomMargin;

  @override
  int get hashCode => Object.hash(
    fontScale,
    font,
    textColor,
    backgroundColor,
    windowColor,
    edge,
    edgeColor,
    bottomMargin,
  );

  @override
  String toString() =>
      'SubtitleAppearance(scale: $fontScale, font: ${font.name}, '
      'text: $textColor, background: $backgroundColor, window: $windowColor, '
      'edge: ${edge.name} $edgeColor, bottom: $bottomMargin)';
}

/// The user's subtitle settings (PLAY-007). By default subtitles follow the
/// system caption style (Android captioning, iOS/tvOS MediaAccessibility);
/// where there is none (desktop) the standard look is used. With
/// [useSystemStyle] off, size, font, text color and background come from
/// here. The position is always the user's: the system styles have none.
@immutable
class SubtitleSettings {
  const SubtitleSettings({
    this.useSystemStyle = true,
    this.scale = 1.0,
    this.font = SubtitleFont.sansSerif,
    this.textColor = const Color(0xFFFFFFFF),
    this.background = SubtitleBackground.none,
    this.position = defaultPosition,
  });

  final bool useSystemStyle;

  /// Text size relative to the default, one of [scales].
  final double scale;
  final SubtitleFont font;
  final Color textColor;
  final SubtitleBackground background;

  /// Distance from the bottom edge as a fraction of the picture height,
  /// [minPosition]..[maxPosition].
  final double position;

  /// Sizes offered in the settings.
  static const List<double> scales = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

  /// Text colors offered in the settings.
  static const List<Color> textColors = [
    Color(0xFFFFFFFF),
    Color(0xFFFFFF00),
    Color(0xFF00FFFF),
    Color(0xFF00FF00),
    Color(0xFFFF00FF),
    Color(0xFF000000),
  ];

  static const double defaultPosition = 0.08;
  static const double minPosition = 0.0;
  static const double maxPosition = 0.4;

  /// Step of the position control.
  static const double positionStep = 0.02;

  SubtitleSettings copyWith({
    bool? useSystemStyle,
    double? scale,
    SubtitleFont? font,
    Color? textColor,
    SubtitleBackground? background,
    double? position,
  }) => SubtitleSettings(
    useSystemStyle: useSystemStyle ?? this.useSystemStyle,
    scale: scale ?? this.scale,
    font: font ?? this.font,
    textColor: textColor ?? this.textColor,
    background: background ?? this.background,
    position: position == null ? this.position : clampPosition(position),
  );

  /// [position] limited to the allowed range and rounded to a step.
  static double clampPosition(double p) {
    final c = p.clamp(minPosition, maxPosition).toDouble();
    return (c / positionStep).round() * positionStep;
  }

  /// The look on screen. [system] is the platform caption style, if the
  /// platform has one ([SystemCaptionStyle.read]).
  SubtitleAppearance resolve(SystemCaptionStyle? system) {
    if (useSystemStyle) {
      final base = system != null && system.enabled
          ? system.appearance()
          : SubtitleAppearance.standard;
      return base.copyWith(bottomMargin: position);
    }
    return SubtitleAppearance(
      fontScale: scale,
      font: font,
      textColor: textColor,
      backgroundColor: switch (background) {
        SubtitleBackground.none => const Color(0x00000000),
        SubtitleBackground.translucent => const Color(0x99000000),
        SubtitleBackground.opaque => const Color(0xFF000000),
      },
      edge: background == SubtitleBackground.none
          ? SubtitleEdge.outline
          : SubtitleEdge.none,
      bottomMargin: position,
    );
  }

  Map<String, Object?> toJson() => {
    'system': useSystemStyle,
    'scale': scale,
    'font': font.name,
    'color': textColor.toARGB32(),
    'background': background.name,
    'position': position,
  };

  /// Reads [toJson] output; missing or invalid values get their defaults.
  factory SubtitleSettings.fromJson(Object? json) {
    if (json is! Map) return const SubtitleSettings();
    T? pick<T extends Enum>(List<T> values, Object? name) =>
        values.where((v) => v.name == name).firstOrNull;
    final scale = json['scale'];
    final position = json['position'];
    final color = json['color'];
    return SubtitleSettings(
      useSystemStyle: json['system'] is bool ? json['system'] as bool : true,
      scale: scale is num && scale > 0 ? scale.toDouble() : 1.0,
      font: pick(SubtitleFont.values, json['font']) ?? SubtitleFont.sansSerif,
      textColor: color is int ? Color(color) : const Color(0xFFFFFFFF),
      background:
          pick(SubtitleBackground.values, json['background']) ??
          SubtitleBackground.none,
      position: position is num
          ? clampPosition(position.toDouble())
          : defaultPosition,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is SubtitleSettings &&
      other.useSystemStyle == useSystemStyle &&
      other.scale == scale &&
      other.font == font &&
      other.textColor == textColor &&
      other.background == background &&
      other.position == position;

  @override
  int get hashCode =>
      Object.hash(useSystemStyle, scale, font, textColor, background, position);
}

/// The caption style of the operating system: Android `CaptioningManager`
/// (read by the pa_player plugin) or Apple MediaAccessibility. Colors are
/// ARGB; a null color means "not set" (the standard one is used).
@immutable
class SystemCaptionStyle {
  const SystemCaptionStyle({
    required this.enabled,
    this.fontScale = 1.0,
    this.foregroundColor,
    this.backgroundColor,
    this.windowColor,
    this.edgeType,
    this.edgeColor,
    this.typeface,
    this.locale,
  });

  /// Captions are switched on in the system settings. Off: the system
  /// style is not applied (as Android's own players do).
  final bool enabled;
  final double fontScale;
  final int? foregroundColor;
  final int? backgroundColor;
  final int? windowColor;

  /// Android `CaptionStyle.EDGE_TYPE_*`: 0 none, 1 outline, 2 drop shadow,
  /// 3 raised, 4 depressed; null or -1: not set.
  final int? edgeType;
  final int? edgeColor;

  /// Android captioning typeface (`serif`, `monospace`, `casual`…) or a
  /// family name; null: default.
  final String? typeface;

  /// Preferred caption language (BCP 47), if the user chose one.
  final String? locale;

  static const MethodChannel _channel = MethodChannel(
    'app.kinolune/captioning',
  );

  /// The current system style, or null where the platform has none
  /// (desktop, tests) or it cannot be read.
  static Future<SystemCaptionStyle?> read() async {
    if (kIsWeb || !(Platform.isAndroid || Platform.isIOS)) return null;
    try {
      final map = await _channel.invokeMapMethod<String, Object?>(
        'captionStyle',
      );
      return map == null ? null : SystemCaptionStyle.fromMap(map);
    } on MissingPluginException {
      return null;
    } on PlatformException catch (e) {
      debugPrint('pa_player: caption style not readable: $e');
      return null;
    }
  }

  /// From the platform channel map (keys as the constructor parameters).
  factory SystemCaptionStyle.fromMap(Map<String, Object?> m) {
    int? color(String key) {
      final v = m[key];
      return v is int ? v & 0xFFFFFFFF : null;
    }

    final scale = m['fontScale'];
    final typeface = m['typeface'];
    final locale = m['locale'];
    final edge = m['edgeType'];
    return SystemCaptionStyle(
      enabled: m['enabled'] == true,
      fontScale: scale is num && scale > 0 ? scale.toDouble() : 1.0,
      foregroundColor: color('foregroundColor'),
      backgroundColor: color('backgroundColor'),
      windowColor: color('windowColor'),
      edgeType: edge is int ? edge : null,
      edgeColor: color('edgeColor'),
      typeface: typeface is String && typeface.isNotEmpty ? typeface : null,
      locale: locale is String && locale.isNotEmpty ? locale : null,
    );
  }

  /// The font of an Android captioning typeface name.
  static SubtitleFont fontOf(String? typeface) => switch (typeface) {
    'serif' => SubtitleFont.serif,
    'monospace' ||
    'sans-serif-monospace' ||
    'serif-monospace' => SubtitleFont.monospace,
    'casual' => SubtitleFont.casual,
    'cursive' => SubtitleFont.cursive,
    'sans-serif-condensed' => SubtitleFont.condensed,
    'sans-serif-smallcaps' => SubtitleFont.smallCaps,
    _ => SubtitleFont.sansSerif,
  };

  /// The look this style gives (position: the default).
  SubtitleAppearance appearance() {
    const s = SubtitleAppearance.standard;
    final edge = switch (edgeType) {
      0 => SubtitleEdge.none,
      1 => SubtitleEdge.outline,
      2 => SubtitleEdge.dropShadow,
      3 => SubtitleEdge.raised,
      4 => SubtitleEdge.depressed,
      _ => s.edge,
    };
    return SubtitleAppearance(
      fontScale: fontScale,
      font: fontOf(typeface),
      textColor: foregroundColor == null ? s.textColor : Color(foregroundColor!),
      backgroundColor: backgroundColor == null
          ? s.backgroundColor
          : Color(backgroundColor!),
      windowColor: windowColor == null ? s.windowColor : Color(windowColor!),
      edge: edge,
      edgeColor: edgeColor == null ? s.edgeColor : Color(edgeColor!),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is SystemCaptionStyle &&
      other.enabled == enabled &&
      other.fontScale == fontScale &&
      other.foregroundColor == foregroundColor &&
      other.backgroundColor == backgroundColor &&
      other.windowColor == windowColor &&
      other.edgeType == edgeType &&
      other.edgeColor == edgeColor &&
      other.typeface == typeface &&
      other.locale == locale;

  @override
  int get hashCode => Object.hash(
    enabled,
    fontScale,
    foregroundColor,
    backgroundColor,
    windowColor,
    edgeType,
    edgeColor,
    typeface,
    locale,
  );

  @override
  String toString() =>
      'SystemCaptionStyle(enabled: $enabled, scale: $fontScale, '
      'fg: $foregroundColor, bg: $backgroundColor, window: $windowColor, '
      'edge: $edgeType/$edgeColor, typeface: $typeface, locale: $locale)';
}
