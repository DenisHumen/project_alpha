// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_design/pa_design.dart';

void main() {
  test('dark theme is the default background from 01 §4.5', () {
    expect(PaColorScheme.dark.bgBase, const Color(0xFF0B0B0F));
    expect(PaColorScheme.light.bgBase, const Color(0xFFF5F5F7));
  });

  test('every density defines the type scale', () {
    expect(PaDensity.tv.typeBody.fontSize, 29);
    expect(PaDensity.phone.typeBody.fontSize, 17);
    expect(PaDensity.values.keys,
        containsAll(<String>['tv', 'desktop', 'tablet', 'phone']));
  });

  test('focus scale and motion', () {
    expect(PaTokens.focusScalePoster, 1.1);
    expect(PaTokens.motionPage.duration, const Duration(milliseconds: 400));
  });

  test('icons have stable ids and SVG data', () {
    expect(PaIcon.values.length, greaterThanOrEqualTo(60));
    expect(PaIcon.play.id, 'play');
    expect(PaIcon.play.svg, startsWith('<svg'));
  });

  testWidgets('PaIconView renders', (tester) async {
    await tester.pumpWidget(const Directionality(
      textDirection: TextDirection.ltr,
      child: Center(child: PaIconView(PaIcon.play, size: 24)),
    ));
    expect(find.byType(PaIconView), findsOneWidget);
  });
}
