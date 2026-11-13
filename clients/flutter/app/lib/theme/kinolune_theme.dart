// SPDX-License-Identifier: MPL-2.0
//
// The whole visual language of the app lives in this one file so it can be
// swapped for generated values from `design/tokens` (DSN-002) without touching
// screens. Values mirror docs/research/01-product-ux-design.md §4.3–4.7
// (dark tvOS-like style) and the token skeleton in design/tokens.

import 'package:material_ui/material_ui.dart';

import '../platform/form_factor.dart';

/// Colour tokens (01 §4.5, dark theme).
abstract final class KColors {
  static const bgBase = Color(0xFF0B0B0F);
  static const bgElevated = Color(0xFF16161C);
  static const bgElevated2 = Color(0xFF1F1F27);
  static const stroke = Color(0x1FFFFFFF);
  static const textPrimary = Color(0xFFFFFFFF);
  static const textSecondary = Color(0x99FFFFFF); // 60 %
  static const textTertiary = Color(0x59FFFFFF); // 35 %

  /// Placeholder brand accent (design/tokens `color.brand.500`), DSN-006.
  static const accent = Color(0xFF3B82F6);
  static const accentSoft = Color(0xFF93B7FA);
  static const live = Color(0xFFEF4444);

  /// TV focused button: white fill, dark label (01 §4.5 focus/*).
  static const focusFill = Color(0xFFFFFFFF);
  static const focusLabel = Color(0xFF000000);
  static const focusGlow = Color(0x663B82F6);

  static const scrimStrong = Color(0xD9000000); // 85 %
}

/// Motion tokens (01 §4.7).
abstract final class KMotion {
  static const focus = Duration(milliseconds: 180);
  static const focusOut = Duration(milliseconds: 140);
  static const press = Duration(milliseconds: 100);
  static const page = Duration(milliseconds: 380);
  static const crossfadeArt = Duration(milliseconds: 650);
  static const focusCurve = Curves.easeOutBack;
  static const pageCurve = Cubic(.2, .8, .2, 1);
}

/// Size/density tokens per form factor (01 §4.3; TV values are the tvOS
/// points divided by 2 because Android TV reports ~960x540 logical pixels).
@immutable
class KDensity {
  const KDensity({
    required this.formFactor,
    required this.pageInsets,
    required this.gap,
    required this.rowGap,
    required this.posterWidth,
    required this.landscapeWidth,
    required this.albumWidth,
    required this.cardRadius,
    required this.panelRadius,
    required this.focusScalePoster,
    required this.focusScaleCard,
    required this.focusScaleButton,
    required this.type,
  });

  final FormFactor formFactor;
  final EdgeInsets pageInsets;
  final double gap;
  final double rowGap;
  final double posterWidth;
  final double landscapeWidth;
  final double albumWidth;
  final double cardRadius;
  final double panelRadius;
  final double focusScalePoster;
  final double focusScaleCard;
  final double focusScaleButton;
  final KType type;

  bool get isTv => formFactor == FormFactor.tv;
  bool get isPhone => formFactor == FormFactor.phone;
  bool get isDesktop => formFactor == FormFactor.desktop;

  static KDensity of(BuildContext context) => forFactor(FormFactorScope.of(context));

  static KDensity forFactor(FormFactor f) => switch (f) {
        FormFactor.tv => tv,
        FormFactor.phone => phone,
        FormFactor.desktop => desktop,
      };

  static const desktop = KDensity(
    formFactor: FormFactor.desktop,
    pageInsets: EdgeInsets.fromLTRB(32, 24, 32, 24),
    gap: 18,
    rowGap: 28,
    posterWidth: 150,
    landscapeWidth: 300,
    albumWidth: 168,
    cardRadius: 12,
    panelRadius: 24,
    focusScalePoster: 1.05,
    focusScaleCard: 1.04,
    focusScaleButton: 1.03,
    type: KType(display: 40, title1: 28, title2: 22, headline: 17, body: 14, callout: 14, caption: 12, caption2: 11),
  );

  static const phone = KDensity(
    formFactor: FormFactor.phone,
    pageInsets: EdgeInsets.fromLTRB(16, 12, 16, 16),
    gap: 12,
    rowGap: 22,
    posterWidth: 112,
    landscapeWidth: 260,
    albumWidth: 140,
    cardRadius: 10,
    panelRadius: 20,
    focusScalePoster: 1.0,
    focusScaleCard: 1.0,
    focusScaleButton: 1.0,
    type: KType(display: 32, title1: 28, title2: 22, headline: 17, body: 15, callout: 15, caption: 13, caption2: 11),
  );

  static const tv = KDensity(
    formFactor: FormFactor.tv,
    // Safe area 60/80 pt at 1920x1080 (HIG) → 30/40 logical px.
    pageInsets: EdgeInsets.fromLTRB(44, 16, 44, 30),
    gap: 20,
    rowGap: 26,
    posterWidth: 118,
    landscapeWidth: 220,
    albumWidth: 130,
    cardRadius: 8,
    panelRadius: 16,
    focusScalePoster: 1.10,
    focusScaleCard: 1.08,
    focusScaleButton: 1.06,
    type: KType(display: 36, title1: 26, title2: 22, headline: 17, body: 13, callout: 14, caption: 12, caption2: 11),
  );
}

@immutable
class KType {
  const KType({
    required this.display,
    required this.title1,
    required this.title2,
    required this.headline,
    required this.body,
    required this.callout,
    required this.caption,
    required this.caption2,
  });
  final double display, title1, title2, headline, body, callout, caption, caption2;
}

/// Font family. Roboto is Apache-2.0 and is the platform font on Android; on
/// Linux fontconfig falls back to the desktop sans font. Inter (DSN-005)
/// replaces it once the font files land in design/.
const kFontFamily = 'Roboto';
const kFontFallback = <String>['Inter', 'Noto Sans', 'Cantarell', 'DejaVu Sans'];

ThemeData buildKinoluneTheme(FormFactor formFactor) {
  final d = KDensity.forFactor(formFactor);
  final t = d.type;
  const scheme = ColorScheme.dark(
    primary: KColors.accent,
    onPrimary: Colors.white,
    secondary: KColors.accentSoft,
    surface: KColors.bgBase,
    surfaceContainerLowest: KColors.bgBase,
    surfaceContainerLow: KColors.bgElevated,
    surfaceContainer: KColors.bgElevated,
    surfaceContainerHigh: KColors.bgElevated2,
    surfaceContainerHighest: KColors.bgElevated2,
    onSurface: KColors.textPrimary,
    onSurfaceVariant: KColors.textSecondary,
    outline: KColors.stroke,
    outlineVariant: KColors.stroke,
    error: KColors.live,
  );
  TextStyle s(double size, FontWeight w, {double? height, Color color = KColors.textPrimary}) =>
      TextStyle(fontSize: size, fontWeight: w, height: height, color: color, letterSpacing: 0);
  final textTheme = TextTheme(
    displayLarge: s(t.display, FontWeight.w700, height: 1.15),
    displayMedium: s(t.display * .85, FontWeight.w700, height: 1.15),
    headlineLarge: s(t.title1, FontWeight.w700, height: 1.2),
    headlineMedium: s(t.title2, FontWeight.w600, height: 1.25),
    headlineSmall: s(t.title2 * .9, FontWeight.w600, height: 1.25),
    titleLarge: s(t.headline, FontWeight.w600, height: 1.3),
    titleMedium: s(t.callout, FontWeight.w600, height: 1.3),
    titleSmall: s(t.caption, FontWeight.w600, height: 1.3),
    bodyLarge: s(t.body + 1, FontWeight.w400, height: 1.45),
    bodyMedium: s(t.body, FontWeight.w400, height: 1.45),
    bodySmall: s(t.caption, FontWeight.w400, height: 1.35, color: KColors.textSecondary),
    labelLarge: s(t.callout, FontWeight.w600),
    labelMedium: s(t.caption, FontWeight.w500, color: KColors.textSecondary),
    labelSmall: s(t.caption2, FontWeight.w500, color: KColors.textSecondary),
  );

  return ThemeData(
    useMaterial3: true,
    brightness: Brightness.dark,
    colorScheme: scheme,
    scaffoldBackgroundColor: KColors.bgBase,
    canvasColor: KColors.bgBase,
    fontFamily: kFontFamily,
    fontFamilyFallback: kFontFallback,
    textTheme: textTheme,
    visualDensity: VisualDensity.standard,
    splashFactory: formFactor == FormFactor.tv ? NoSplash.splashFactory : InkSparkle.splashFactory,
    focusColor: Colors.white.withValues(alpha: .12),
    hoverColor: Colors.white.withValues(alpha: .06),
    dividerTheme: const DividerThemeData(color: KColors.stroke, thickness: 1, space: 1),
    appBarTheme: AppBarTheme(
      backgroundColor: Colors.transparent,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      titleTextStyle: textTheme.titleLarge,
    ),
    cardTheme: CardThemeData(
      color: KColors.bgElevated,
      elevation: 0,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(d.cardRadius)),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: KColors.bgElevated,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(d.panelRadius)),
      titleTextStyle: textTheme.headlineSmall,
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: Colors.white.withValues(alpha: .06),
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: BorderSide.none),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(12),
        borderSide: const BorderSide(color: KColors.accent, width: 2),
      ),
      labelStyle: textTheme.bodyMedium?.copyWith(color: KColors.textSecondary),
      hintStyle: textTheme.bodyMedium?.copyWith(color: KColors.textTertiary),
    ),
    chipTheme: ChipThemeData(
      backgroundColor: Colors.white.withValues(alpha: .06),
      selectedColor: Colors.white,
      labelStyle: textTheme.labelLarge?.copyWith(fontSize: t.caption + 1),
      secondaryLabelStyle: textTheme.labelLarge?.copyWith(fontSize: t.caption + 1, color: Colors.black),
      side: BorderSide.none,
      shape: const StadiumBorder(),
      showCheckmark: false,
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: KColors.bgElevated.withValues(alpha: .96),
      indicatorColor: KColors.accent.withValues(alpha: .22),
      surfaceTintColor: Colors.transparent,
      labelTextStyle: WidgetStatePropertyAll(textTheme.labelSmall),
      height: 64,
    ),
    progressIndicatorTheme: const ProgressIndicatorThemeData(
      color: KColors.accent,
      linearTrackColor: Color(0x33FFFFFF),
    ),
    snackBarTheme: SnackBarThemeData(
      backgroundColor: KColors.bgElevated2,
      contentTextStyle: textTheme.bodyMedium,
      behavior: SnackBarBehavior.floating,
    ),
    listTileTheme: ListTileThemeData(
      iconColor: KColors.textSecondary,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
    ),
    scrollbarTheme: ScrollbarThemeData(
      thumbColor: WidgetStatePropertyAll(Colors.white.withValues(alpha: .18)),
    ),
    pageTransitionsTheme: const PageTransitionsTheme(
      builders: {
        TargetPlatform.android: FadeForwardsPageTransitionsBuilder(),
        TargetPlatform.linux: FadeForwardsPageTransitionsBuilder(),
      },
    ),
  );
}
