// SPDX-License-Identifier: MPL-2.0
//
// UI-005: the Sources screen, the add flow and its forms (local folder, SMB,
// WebDAV), and readable error texts from the core's error codes.

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_source_manager.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/l10n/l10n.dart';
import 'package:kinolune/screens/sources_screen.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/sources/discovery.dart';
import 'package:kinolune/sources/source_address.dart';
import 'package:kinolune/sources/source_manager.dart';
import 'package:kinolune/sources/source_text.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

final en = lookupAppLocalizations(const Locale('en'));
final ru = lookupAppLocalizations(const Locale('ru'));

FakeSourceManager _fake(AppServices s) => s.sourceManager as FakeSourceManager;

Future<AppServices> _openAdd(
  WidgetTester tester, {
  bool reduced = false,
  SourceDiscovery discovery = const NoSourceDiscovery(),
}) async {
  setSurface(tester, const Size(1280, 900));
  final services = await demoServices(
    reducedSources: reduced,
    discovery: discovery,
  );
  await tester.pumpWidget(testApp(services, section: Section.settings));
  await settle(tester);
  await tester.tap(find.text(en.addSourceTitle).first);
  await settle(tester, 4);
  return services;
}

Future<void> _pick(WidgetTester tester, String kind) async {
  await tester.tap(find.text(kind));
  await settle(tester, 4);
}

Future<void> _enter(WidgetTester tester, String key, String text) async {
  await tester.ensureVisible(find.byKey(ValueKey(key)));
  await tester.enterText(find.byKey(ValueKey(key)), text);
  await tester.pump();
}

Future<void> _tapText(WidgetTester tester, String text) async {
  await tester.ensureVisible(find.text(text));
  await tester.pump();
  await tester.tap(find.text(text));
  await settle(tester, 4);
}

Future<void> _submit(WidgetTester tester) async {
  await tester.ensureVisible(find.byKey(const ValueKey('source-submit')));
  await tester.tap(find.byKey(const ValueKey('source-submit')));
  await settle(tester, 8);
}

Future<void> _openSourcesScreen(WidgetTester tester) async {
  await tester.ensureVisible(find.text(en.sourcesManage));
  await settle(tester, 2);
  await tester.tap(find.text(en.sourcesManage));
  await settle(tester, 8);
}

void main() {
  setUpAll(loadAppFonts);

  group('error codes → readable text', () {
    test('bridge codes', () {
      expect(
        classifySourceError('not_found', 'source 4 not found'),
        SourceErrorCode.sourceMissing,
      );
      expect(
        classifySourceError('invalid', 'invalid source: x'),
        SourceErrorCode.invalidSettings,
      );
      expect(
        classifySourceError('secrets', 'secret store: locked'),
        SourceErrorCode.secretStore,
      );
    });

    test('pa-vfs kinds inside the message', () {
      const cases = {
        'source 2: authentication failed: STATUS_LOGON_FAILURE':
            SourceErrorCode.authFailed,
        'source 2: permission denied: /x': SourceErrorCode.permissionDenied,
        'source 2: source unavailable: connection refused':
            SourceErrorCode.unreachable,
        'source 2: timed out: 30 s': SourceErrorCode.timedOut,
        'source 2: not found: /media/Movies': SourceErrorCode.pathNotFound,
        'source 2: not a directory: /a.mkv': SourceErrorCode.notADirectory,
        'source 2: invalid path: a<b': SourceErrorCode.invalidPath,
        'source 2: unsupported: libsmb2 not found': SourceErrorCode.unsupported,
        'source 2: error: boom': SourceErrorCode.other,
      };
      for (final MapEntry(:key, :value) in cases.entries) {
        expect(classifySourceError('unreachable', key), value, reason: key);
      }
    });

    test('stored status errors (check and scan)', () {
      expect(
        classifySourceMessage('authentication failed: bad password'),
        SourceErrorCode.authFailed,
      );
      expect(
        classifySourceMessage('folder not found: /mnt/usb'),
        SourceErrorCode.pathNotFound,
      );
      expect(
        classifySourceMessage('no local path for this source'),
        SourceErrorCode.notIndexable,
      );
      // The earliest kind wins over words in the path.
      expect(
        classifySourceMessage('timed out: /not found/x'),
        SourceErrorCode.timedOut,
      );
    });

    test('every code has its own text in ru and en', () {
      for (final l in [en, ru]) {
        final texts = {
          for (final c in SourceErrorCode.values) sourceErrorText(l, c),
        };
        expect(texts, hasLength(SourceErrorCode.values.length));
        expect(texts.every((t) => t.length > 20), isTrue);
      }
      expect(
        sourceErrorText(ru, SourceErrorCode.authFailed),
        'Неверное имя пользователя или пароль, либо у пользователя нет '
        'доступа к папке.',
      );
      for (final e in SourceFormError.values) {
        expect(sourceFormErrorText(ru, e), isNotEmpty);
      }
    });

    test('an error is current until a later success', () {
      final t0 = DateTime(2026, 11, 14, 10);
      final failed = SourceStatusInfo(
        online: false,
        checkedAt: t0,
        lastError: 'timed out: x',
        lastErrorAt: t0,
      );
      expect(failed.currentError, 'timed out: x');
      final rechecked = SourceStatusInfo(
        online: true,
        checkedAt: t0.add(const Duration(minutes: 1)),
        lastError: 'timed out: x',
        lastErrorAt: t0,
      );
      expect(rechecked.currentError, isNull);
      final rescanned = SourceStatusInfo(
        lastScanStartedAt: t0.add(const Duration(minutes: 2)),
        lastError: 'folder not found: /a',
        lastErrorAt: t0,
      );
      expect(rescanned.currentError, isNull);
    });

    test('speeds and relative times', () {
      expect(formatSpeed(en, 12300000), '12.3 MB/s');
      expect(formatSpeed(ru, 12300000), '12,3 МБ/с');
      expect(formatSpeed(en, 850000), '850.0 KB/s');
      final now = DateTime(2026, 11, 14, 12);
      String ago(Duration d) => formatAgo(ru, now.subtract(d), now: now);
      expect(ago(const Duration(seconds: 10)), 'только что');
      expect(ago(const Duration(minutes: 5)), '5 минут назад');
      expect(ago(const Duration(hours: 2)), '2 часа назад');
      expect(ago(const Duration(days: 21)), '21 день назад');
    });
  });

  group('form rules', () {
    test('local folder', () {
      expect(localAddress('  ').error, SourceFormError.folderEmpty);
      expect(localAddress('Videos').error, SourceFormError.folderNotAbsolute);
      expect(localAddress('/srv/media ').address, '/srv/media');
      expect(localAddress(r'D:\Movies').address, r'D:\Movies');
    });

    test('SMB address from fields or a pasted address', () {
      expect(
        smbAddress(host: 'nas.local', share: '/media/', folder: 'Movies/').address,
        'smb://nas.local/media/Movies',
      );
      expect(
        smbAddress(host: r'\\nas\media\Kids', share: '').address,
        'smb://nas/media/Kids',
      );
      expect(
        smbAddress(host: 'smb://nas/media', share: '').address,
        'smb://nas/media',
      );
      expect(smbAddress(host: '', share: 'x').error, SourceFormError.hostEmpty);
      expect(smbAddress(host: 'nas', share: '').error, SourceFormError.shareEmpty);
      expect(
        smbAddress(host: 'anna@nas', share: 'x').error,
        SourceFormError.credentialsInAddress,
      );
    });

    test('WebDAV URL and certificate pin', () {
      expect(webdavAddress('nas.local/dav').address, 'https://nas.local/dav');
      expect(webdavAddress('http://nas:8080/').address, 'http://nas:8080/');
      expect(webdavAddress('').error, SourceFormError.urlEmpty);
      expect(webdavAddress('ftp://nas/x').error, SourceFormError.urlScheme);
      expect(
        webdavAddress('https://anna:pw@nas/dav').error,
        SourceFormError.credentialsInAddress,
      );
      expect(tlsPin(''), isNull);
      final hex = List.filled(32, 'aB').join();
      expect(tlsPin('SHA256:$hex')!.address, 'sha256:${hex.toLowerCase()}');
      final colons = List.filled(32, 'ab').join(':');
      expect(tlsPin(colons)!.address, 'sha256:${'ab' * 32}');
      expect(tlsPin('abc')!.error, SourceFormError.tlsPin);
    });

    test('suggested names', () {
      expect(suggestSourceName('/home/me/Videos/'), 'Videos');
      expect(suggestSourceName('smb://nas/media/Movies'), 'Movies');
      expect(suggestSourceName('https://nas.local/'), 'nas.local');
      expect(suggestSourceName(r'C:\Media\Films'), 'Films');
    });
  });

  group('add flow', () {
    testWidgets('lists manual kinds; only folder, SMB and WebDAV are enabled', (
      tester,
    ) async {
      await _openAdd(tester);
      ListTile tile(String text) => tester.widget<ListTile>(
        find.ancestor(of: find.text(text), matching: find.byType(ListTile)),
      );
      for (final t in [en.sourcesTypeLocal, en.sourcesTypeSmb, en.sourcesTypeWebdav]) {
        expect(tile(t).enabled, isTrue, reason: t);
      }
      for (final t in [
        en.sourcesTypeNfs,
        en.sourcesTypeSftp,
        en.sourcesTypeFtp,
        en.sourcesTypePlex,
        en.sourcesTypeJellyfin,
        en.sourcesTypeEmby,
      ]) {
        expect(tile(t).enabled, isFalse, reason: t);
      }
      expect(find.text(en.sourcesSoon), findsNWidgets(6));
      // No discovery backend yet: no "On this network" section.
      expect(find.text(en.sourcesAddDiscovered), findsNothing);
    });

    testWidgets('discovered shares come first and prefill the form', (
      tester,
    ) async {
      await _openAdd(
        tester,
        discovery: const StaticSourceDiscovery([
          DiscoveredSource(
            type: SourceType.smb,
            name: 'Living room NAS',
            host: 'nas.local',
          ),
        ]),
      );
      final found = tester.getTopLeft(find.text('Living room NAS')).dy;
      final manual = tester.getTopLeft(find.text(en.sourcesAddManual)).dy;
      expect(found, lessThan(manual));
      await _pick(tester, 'Living room NAS');
      final host = tester.widget<TextField>(
        find.byKey(const ValueKey('source-host')),
      );
      expect(host.controller!.text, 'nas.local');
    });

    testWidgets('folder form validates the path', (tester) async {
      final services = await _openAdd(tester);
      await _pick(tester, en.sourcesTypeLocal);
      await _submit(tester);
      expect(find.text(en.addSourceErrorNoFolder), findsOneWidget);
      await _enter(tester, 'source-folder', 'Videos');
      await _submit(tester);
      expect(find.text(en.sourcesFormErrorFolderNotAbsolute), findsOneWidget);
      await _enter(tester, 'source-folder', '/srv/films/');
      await _submit(tester);
      final draft = _fake(services).drafts.single;
      expect(draft.type, SourceType.local);
      expect(draft.address, '/srv/films/');
      expect(draft.name, 'films');
      expect(draft.password, isNull);
      expect(services.sources.any((s) => s.path == '/srv/films/'), isTrue);
    });

    testWidgets('SMB form: address, login, options, password in keystore', (
      tester,
    ) async {
      final services = await _openAdd(tester);
      await _pick(tester, en.sourcesTypeSmb);
      await _submit(tester);
      expect(find.text(en.sourcesFormErrorHostEmpty), findsOneWidget);
      await _enter(tester, 'source-host', 'nas.local');
      await _submit(tester);
      expect(find.text(en.sourcesFormErrorShareEmpty), findsOneWidget);
      await _enter(tester, 'source-share', 'media');
      await _enter(tester, 'source-subfolder', 'Movies');
      expect(
        find.text(en.sourcesFormAddress('smb://nas.local/media/Movies')),
        findsOneWidget,
      );
      await _enter(tester, 'source-user', 'viewer');
      await _enter(tester, 'source-password', 's3cret');
      await _tapText(tester, en.sourcesFormAdvanced);
      await _enter(tester, 'source-domain', 'HOME');
      await _tapText(tester, en.sourcesFormSmbVersionSmb3);
      await _tapText(tester, en.sourcesFormEncryptionRequired);
      await _submit(tester);

      final d = _fake(services).drafts.single;
      expect(d.type, SourceType.smb);
      expect(d.address, 'smb://nas.local/media/Movies');
      expect(d.name, 'Movies');
      expect(d.username, 'viewer');
      expect(d.password, 's3cret');
      expect(d.smb.domain, 'HOME');
      expect(d.smb.version, 'smb3');
      expect(d.smb.encryption, 'required');
      expect(d.toString(), isNot(contains('s3cret')));
      final smb = services.sources.firstWhere((s) => s.kind == SourceKind.smb);
      expect(
        await services.secureStore.read('smb_password_${smb.id}'),
        's3cret',
      );
    });

    testWidgets('WebDAV form: URL and pin rules; not indexed yet', (
      tester,
    ) async {
      final services = await _openAdd(tester);
      await _pick(tester, en.sourcesTypeWebdav);
      expect(find.text(en.sourcesNotIndexable), findsOneWidget);
      expect(find.text(en.sourcesFormAdd), findsOneWidget); // no "and scan"
      await _enter(tester, 'source-url', 'https://anna:pw@nas/dav');
      await _submit(tester);
      expect(find.text(en.sourcesFormErrorCredentials), findsOneWidget);
      await _enter(tester, 'source-url', 'nas.local/dav/media');
      await _tapText(tester, en.sourcesFormAdvanced);
      await _enter(tester, 'source-tls-pin', '12:34');
      await _submit(tester);
      expect(find.text(en.sourcesFormErrorTlsPin), findsOneWidget);
      expect(_fake(services).drafts, isEmpty);
      await _enter(tester, 'source-tls-pin', 'f' * 64);
      await _submit(tester);
      final d = _fake(services).drafts.single;
      expect(d.type, SourceType.webdav);
      expect(d.address, 'https://nas.local/dav/media');
      expect(d.tlsPin, 'sha256:${'f' * 64}');
      expect(d.scanIntoLibrary, isFalse);
      expect(services.scanning, isFalse);
    });

    testWidgets('a core error is shown as a readable sentence', (tester) async {
      final services = await _openAdd(tester);
      _fake(services).nextError = SourceException.fromCore(
        'unreachable',
        'source 9: authentication failed: STATUS_LOGON_FAILURE',
      );
      await _pick(tester, en.sourcesTypeSmb);
      await _enter(tester, 'source-host', 'nas');
      await _enter(tester, 'source-share', 'media');
      await _submit(tester);
      expect(find.text(en.sourcesErrorAuth), findsOneWidget);
      expect(find.text(en.sourcesErrorDetails), findsOneWidget);
      // The dialog stays open for a fix.
      expect(find.byKey(const ValueKey('source-submit')), findsOneWidget);
    });

    testWidgets('simplified mode: only local folders', (tester) async {
      await _openAdd(tester, reduced: true);
      final smb = tester.widget<ListTile>(
        find.ancestor(
          of: find.text(en.sourcesTypeSmb),
          matching: find.byType(ListTile),
        ),
      );
      expect(smb.enabled, isFalse);
      expect(find.text(en.sourcesUnavailableReduced), findsNWidgets(2));
      await _pick(tester, en.sourcesTypeLocal);
      expect(find.byKey(const ValueKey('source-folder')), findsOneWidget);
    });
  });

  group('Sources screen', () {
    testWidgets('status, last scan, readable error, rescan', (tester) async {
      setSurface(tester, const Size(1280, 900));
      final services = await demoServices();
      await tester.pumpWidget(testApp(services, section: Section.settings));
      await settle(tester);
      await _openSourcesScreen(tester);
      expect(find.text(en.sourcesTitle), findsOneWidget);
      expect(find.text('Demo library'), findsOneWidget);
      expect(find.text(en.sourcesStatusOnline), findsOneWidget);
      expect(find.text(en.sourcesNeverScanned), findsOneWidget);

      await tester.tap(find.text(en.sourcesRescan));
      await settle(tester, 20);
      expect(services.scanProgress?.isDone, isTrue);
      expect(
        find.text(en.sourcesLastScan(en.sourcesAgoJustNow)),
        findsOneWidget,
      );

      _fake(services).nextError = const SourceException(
        SourceErrorCode.timedOut,
        'timed out: no answer from nas',
      );
      await tester.tap(find.text(en.sourcesCheck));
      await settle(tester, 6);
      expect(find.text(en.sourcesStatusOffline), findsOneWidget);
      expect(find.text(en.sourcesErrorTimedOut), findsOneWidget);
    });

    testWidgets('speed test shows the speed in plain words', (tester) async {
      setSurface(tester, const Size(1280, 900));
      final services = await demoServices();
      await tester.pumpWidget(testApp(services, section: Section.settings));
      await settle(tester);
      await _openSourcesScreen(tester);
      await tester.tap(find.text(en.sourcesSpeedTest));
      await settle(tester, 6);
      expect(find.byType(SpeedTestDialog), findsOneWidget);
      expect(find.text(en.sourcesSpeedRead('32.0 MB/s')), findsOneWidget);
      expect(
        find.text(en.sourcesSpeedResponse(en.sourcesLatencyMs(4))),
        findsOneWidget,
      );
      await tester.tap(find.text(en.sourcesClose));
      await settle(tester, 4);
      expect(
        find.text(en.sourcesSpeedLine('32.0 MB/s', en.sourcesLatencyMs(4))),
        findsOneWidget,
      );
    });

    testWidgets('edit keeps the password unless it is replaced', (
      tester,
    ) async {
      setSurface(tester, const Size(1280, 900));
      final services = await demoServices(empty: true);
      final nas = await services.createSource(
        const SourceDraft(
          type: SourceType.smb,
          name: 'NAS',
          address: 'smb://nas/media',
          username: 'anna',
          password: 'old-pass',
        ),
      );
      await tester.pumpWidget(testApp(services, section: Section.settings));
      await settle(tester);
      await _openSourcesScreen(tester);
      await tester.tap(find.text(en.sourcesEdit));
      await settle(tester, 4);
      expect(find.text(en.sourcesEditTitle), findsOneWidget);
      expect(find.text(en.sourcesFormPasswordKeep), findsOneWidget);
      await _enter(tester, 'source-name', 'Home NAS');
      await _submit(tester);
      final d = _fake(services).drafts.last;
      expect(d.name, 'Home NAS');
      expect(d.address, 'smb://nas/media');
      expect(d.password, isNull); // keep
      expect(
        await services.secureStore.read('smb_password_${nas.id}'),
        'old-pass',
      );
      expect(find.text('Home NAS'), findsWidgets);
    });

    testWidgets('remove asks first and drops the source', (tester) async {
      setSurface(tester, const Size(1280, 900));
      final services = await demoServices();
      await tester.pumpWidget(testApp(services, section: Section.settings));
      await settle(tester);
      await _openSourcesScreen(tester);
      await tester.tap(find.text(en.settingsRemove));
      await settle(tester, 4);
      expect(find.text(en.settingsRemoveSourceTitle), findsOneWidget);
      await tester.tap(find.widgetWithText(FilledButton, en.settingsRemove));
      await settle(tester, 6);
      expect(services.sources, isEmpty);
      expect(find.text(en.sourcesEmpty), findsOneWidget);
    });

    testWidgets('simplified mode banner and no speed test or edit', (
      tester,
    ) async {
      setSurface(tester, const Size(1280, 900));
      final services = await demoServices(reducedSources: true);
      await tester.pumpWidget(testApp(services, section: Section.settings));
      await settle(tester);
      await _openSourcesScreen(tester);
      expect(find.text(en.sourcesReducedBanner('Kinolune')), findsOneWidget);
      expect(find.text(en.sourcesSpeedTest), findsNothing);
      expect(find.text(en.sourcesEdit), findsNothing);
      expect(find.text(en.sourcesRescan), findsOneWidget);
      // The demo folder does not exist on this machine.
      expect(find.text(en.sourcesStatusOffline), findsOneWidget);
      expect(find.text(en.sourcesErrorPathNotFound), findsOneWidget);
    });
  });
}
