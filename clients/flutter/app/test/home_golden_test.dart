// SPDX-License-Identifier: MPL-2.0
//
// Golden images of Home (UI-007) on the four densities: phone, tablet,
// desktop, TV. Like test/navigation_test.dart they use the default test
// font (no loadAppFonts), so they do not depend on fonts installed on the
// machine. Refresh deliberately:
//   flutter test --update-goldens test/home_golden_test.dart

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

void main() {
  final densities = <(String, FormFactor, Size)>[
    ('phone', FormFactor.phone, const Size(390, 844)),
    ('tablet', FormFactor.phone, const Size(820, 1180)),
    ('desktop', FormFactor.desktop, const Size(1280, 800)),
    ('tv', FormFactor.tv, const Size(960, 540)),
  ];
  for (final (name, ff, size) in densities) {
    testWidgets('home on $name', (t) async {
      setSurface(t, size);
      final services = await demoServices();
      await t.pumpWidget(testApp(services, formFactor: ff));
      await settle(t);
      await expectLater(
        find.byKey(screenshotKey),
        matchesGoldenFile('goldens/home_$name.png'),
      );
      await t.pumpWidget(const SizedBox());
      await settle(t, 2);
    });
  }
}
