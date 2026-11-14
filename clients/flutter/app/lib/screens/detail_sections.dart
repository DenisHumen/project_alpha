// SPDX-License-Identifier: MPL-2.0
//
// Parts of the movie Detail screen (UI-009, 01 §2.2): technical badges,
// external ratings, cast and crew, extras, the collection row and the
// technical information of the file. Badges are plain text chips with a
// white outline (01 §4.5 badge/*): no Dolby or DTS logos.

import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/artwork.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';

// ------------------------------------------------------------------ badges

/// Resolution class of a frame: `4K`, `1080p`, `720p`, `SD`.
String? resolutionLabel(int? width, int? height) {
  final w = width ?? 0, h = height ?? 0;
  if (w <= 0 && h <= 0) return null;
  if (w >= 3200 || h >= 2000) return '4K'; // l10n-ignore
  if (w >= 1800 || h >= 1000) return '1080p'; // l10n-ignore
  if (w >= 1200 || h >= 700) return '720p'; // l10n-ignore
  return 'SD'; // l10n-ignore
}

String hdrLabel(HdrFormat f) => switch (f) {
  HdrFormat.dolbyVision => 'Dolby Vision', // l10n-ignore
  HdrFormat.hdr10Plus => 'HDR10+', // l10n-ignore
  HdrFormat.hdr10 => 'HDR10', // l10n-ignore
  HdrFormat.hlg => 'HLG', // l10n-ignore
};

/// Text of an audio format slug of pa-probe, null for formats not worth a
/// badge (AAC, MP3…).
String? audioLabel(String slug) => switch (slug) {
  'atmos' => 'Atmos', // l10n-ignore
  'dts_x' => 'DTS:X', // l10n-ignore
  'truehd' => 'TrueHD', // l10n-ignore
  'dts_hd_ma' => 'DTS-HD MA', // l10n-ignore
  'dts_hd_hra' => 'DTS-HD', // l10n-ignore
  'dts' => 'DTS', // l10n-ignore
  'eac3' => 'DD+', // l10n-ignore
  'ac3' => 'DD', // l10n-ignore
  'flac' => 'FLAC', // l10n-ignore
  'pcm' => 'PCM', // l10n-ignore
  _ => null,
};

/// Badges of the file an item plays: resolution, the strongest HDR format
/// and the best audio (object audio first). Without probe data, what the
/// release name of the default version says.
List<String> techBadges(TechInfo? tech, MediaVersion? version) {
  final out = <String>[];
  final res = resolutionLabel(tech?.width, tech?.height) ?? version?.resolution;
  if (res != null && res.isNotEmpty) out.add(res);
  final hdr = tech?.hdr ?? const <HdrFormat>[];
  if (hdr.isNotEmpty) {
    out.add(hdrLabel(hdr.first));
  } else if (version?.hdr case final h? when h.isNotEmpty) {
    out.add(h.toUpperCase() == 'DV' ? hdrLabel(HdrFormat.dolbyVision) : h);
  }
  final audio = tech?.audioFormats ?? const <String>[];
  final object = audio.where((a) => a == 'atmos' || a == 'dts_x');
  final codec = audio
      .map(audioLabel)
      .nonNulls
      .where(
        (a) => a != 'Atmos' && a != 'DTS:X', // l10n-ignore
      );
  if (object.isNotEmpty) out.add(audioLabel(object.first)!);
  if (codec.isNotEmpty) out.add(codec.first);
  return out;
}

/// A monochrome text badge with a white outline.
class TechBadge extends StatelessWidget {
  const TechBadge(this.text, {super.key});
  final String text;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return Container(
      padding: EdgeInsets.symmetric(
        horizontal: d.isTv ? 8 : 6,
        vertical: d.isTv ? 3 : 2,
      ),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(4),
        border: Border.all(color: Colors.white.withValues(alpha: .7)),
      ),
      child: Text(
        text,
        style: TextStyle(
          color: Colors.white,
          fontSize: d.type.caption2 + 1,
          fontWeight: FontWeight.w600,
          letterSpacing: .4,
          height: 1.2,
        ),
      ),
    );
  }
}

// ----------------------------------------------------------------- ratings

String _ratingSource(String source) => switch (source) {
  'tmdb' => 'TMDB', // l10n-ignore
  'imdb' => 'IMDb', // l10n-ignore
  _ => source.toUpperCase(),
};

/// `TMDB 7.8` per rating; hidden when there are none.
class RatingsRow extends StatelessWidget {
  const RatingsRow({super.key, required this.ratings});
  final List<ExternalRating> ratings;

  @override
  Widget build(BuildContext context) {
    if (ratings.isEmpty) return const SizedBox.shrink();
    final d = KDensity.of(context);
    final l = context.l10n;
    final locale = Localizations.localeOf(context).toLanguageTag();
    final f = NumberFormat('0.0', locale);
    final style = Theme.of(context).textTheme.labelMedium
        ?.copyWith(fontSize: d.type.caption + 1);
    return Wrap(
      key: const ValueKey('detail-ratings'),
      spacing: 16,
      runSpacing: 6,
      children: [
        for (final r in ratings)
          Semantics(
            label: r.votes == null ? null : l.detailRatingVotes(r.votes!),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(
                  Icons.star_rounded,
                  size: d.type.caption + 4,
                  color: KColors.accent,
                ),
                const SizedBox(width: 4),
                Text(
                  '${_ratingSource(r.source)} ${f.format(r.value)}',
                  style: style,
                ),
              ],
            ),
          ),
      ],
    );
  }
}

// --------------------------------------------------------------- sections

/// Section heading below the hero.
class DetailSection extends StatelessWidget {
  const DetailSection({super.key, required this.title, required this.child});
  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return Padding(
      padding: EdgeInsets.only(top: d.isTv ? 28 : 32),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: EdgeInsets.only(
              left: d.pageInsets.left,
              right: d.pageInsets.right,
              bottom: d.isTv ? 6 : 10,
            ),
            child: Text(
              title,
              style: Theme.of(context).textTheme.titleLarge,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          child,
        ],
      ),
    );
  }
}

/// Crew job in the user's language, else as the provider writes it.
String jobLabel(AppLocalizations l, String job) => switch (job) {
  'Director' => l.detailJobDirector, // l10n-ignore
  'Screenplay' => l.detailJobScreenplay, // l10n-ignore
  'Writer' => l.detailJobWriter, // l10n-ignore
  'Novel' => l.detailJobNovel, // l10n-ignore
  'Original Music Composer' => l.detailJobComposer, // l10n-ignore
  'Director of Photography' => l.detailJobCinematography, // l10n-ignore
  _ => job,
};

/// Cast and crew as round photos with name and role. People pages are P1:
/// [onPerson] is the hook for them (null: the cards only take focus).
class CreditsRow extends StatelessWidget {
  const CreditsRow({super.key, required this.credits, this.onPerson});
  final List<CreditPerson> credits;
  final ValueChanged<CreditPerson>? onPerson;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final theme = Theme.of(context);
    final size = d.isTv ? 96.0 : (d.isPhone ? 76.0 : 92.0);
    final width = size + 28;
    return SizedBox(
      height: size + (d.isTv ? 20 : 14) + d.type.caption * 3.2 + 8,
      child: FocusTraversalGroup(
        child: ListView.separated(
          key: const ValueKey('detail-credits'),
          scrollDirection: Axis.horizontal,
          clipBehavior: Clip.none,
          padding: EdgeInsets.only(
            left: d.pageInsets.left,
            right: d.pageInsets.right,
            top: d.isTv ? 10 : 4,
          ),
          itemCount: credits.length,
          separatorBuilder: (_, _) => SizedBox(width: d.gap * .5),
          itemBuilder: (context, i) {
            final p = credits[i];
            final role = p.crew
                ? jobLabel(l, p.job ?? '')
                : (p.character ?? '');
            return SizedBox(
              width: width,
              child: Column(
                children: [
                  KFocusable(
                    onActivate: onPerson == null ? null : () => onPerson!(p),
                    radius: size / 2,
                    focusScale: d.focusScaleCard,
                    semanticLabel: role.isEmpty ? p.name : '${p.name}, $role',
                    builder: (context, _) => SizedBox.square(
                      dimension: size,
                      child: ClipOval(child: _Avatar(person: p)),
                    ),
                  ),
                  SizedBox(height: d.isTv ? 10 : 8),
                  Text(
                    p.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    textAlign: TextAlign.center,
                    style: theme.textTheme.titleSmall,
                  ),
                  if (role.isNotEmpty)
                    Text(
                      role,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      textAlign: TextAlign.center,
                      style: theme.textTheme.labelSmall,
                    ),
                ],
              ),
            );
          },
        ),
      ),
    );
  }
}

class _Avatar extends StatelessWidget {
  const _Avatar({required this.person});
  final CreditPerson person;

  @override
  Widget build(BuildContext context) {
    final url = person.photoUrl;
    if (url != null) {
      return Artwork(url: url, seed: person.name, style: ArtStyle.album);
    }
    final initials = person.name
        .split(RegExp(r'\s+'))
        .where((w) => w.isNotEmpty)
        .take(2)
        .map((w) => w.characters.first.toUpperCase())
        .join();
    return ColoredBox(
      color: KColors.bgElevated2,
      child: Center(
        child: Text(
          initials,
          style: Theme.of(context).textTheme.titleLarge
              ?.copyWith(color: KColors.textSecondary),
        ),
      ),
    );
  }
}

String extraKindLabel(AppLocalizations l, MediaExtraKind k) => switch (k) {
  MediaExtraKind.trailer => l.detailExtraTrailer,
  MediaExtraKind.teaser => l.detailExtraTeaser,
  MediaExtraKind.featurette => l.detailExtraFeaturette,
  MediaExtraKind.behindTheScenes => l.detailExtraBehindTheScenes,
  MediaExtraKind.deletedScene => l.detailExtraDeletedScene,
  MediaExtraKind.interview => l.detailExtraInterview,
  MediaExtraKind.short => l.detailExtraShort,
  MediaExtraKind.scene => l.detailExtraScene,
  MediaExtraKind.other => l.detailExtraOther,
};

/// Extras as 16:9 cards; they play without saving progress.
class ExtrasRow extends StatelessWidget {
  const ExtrasRow({super.key, required this.item, required this.extras});
  final LibraryItem item;
  final List<MediaExtra> extras;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final width = d.landscapeWidth;
    return SizedBox(
      key: const ValueKey('detail-extras'),
      height: width * 9 / 16 + cardCaptionHeight(d) + (d.isTv ? 14 : 8),
      child: FocusTraversalGroup(
        child: ListView.separated(
          scrollDirection: Axis.horizontal,
          clipBehavior: Clip.none,
          padding: EdgeInsets.only(
            left: d.pageInsets.left,
            right: d.pageInsets.right,
            top: d.isTv ? 10 : 4,
          ),
          itemCount: extras.length,
          separatorBuilder: (_, _) => SizedBox(width: d.gap),
          itemBuilder: (context, i) {
            final e = extras[i];
            return LandscapeCard(
              seed: '${item.id}/${e.title}',
              title: e.title,
              subtitle: [
                extraKindLabel(l, e.kind),
                formatRuntime(l, e.durationMs),
              ].where((s) => s.isNotEmpty).join('  ·  '),
              imageUrl: item.backdropUrl,
              width: width,
              onTap: () => playExtra(context, item, e),
            );
          },
        ),
      ),
    );
  }
}

/// Plays an extra of [item] in the fullscreen player, without progress.
Future<void> playExtra(
  BuildContext context,
  LibraryItem item,
  MediaExtra extra,
) async {
  final services = Services.read(context);
  services.music.pause();
  await Navigator.of(context, rootNavigator: true).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) => PlayerScreen(
        request: PlayRequest(
          uri: extra.playbackUri,
          title: '${item.title}  ${extra.title}',
          artworkUri: item.backdropUrl ?? item.posterUrl,
          durationMs: extra.durationMs,
        ),
        engineFactory: services.playerEngineFactory,
        onProgress: (_, _) {},
        subtitleSettings: services.subtitleSettings,
        subtitleLanguages: services.subtitleLanguages,
      ),
    ),
  );
}

/// The other movies of a collection as posters.
class CollectionRow extends StatelessWidget {
  const CollectionRow({
    super.key,
    required this.items,
    required this.current,
    required this.onOpen,
  });
  final List<LibraryItem> items;
  final String current;
  final ValueChanged<LibraryItem> onOpen;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final width = d.posterWidth;
    return SizedBox(
      key: const ValueKey('detail-collection'),
      height: width * 3 / 2 + cardCaptionHeight(d) + (d.isTv ? 14 : 8),
      child: FocusTraversalGroup(
        child: ListView.separated(
          scrollDirection: Axis.horizontal,
          clipBehavior: Clip.none,
          padding: EdgeInsets.only(
            left: d.pageInsets.left,
            right: d.pageInsets.right,
            top: d.isTv ? 10 : 4,
          ),
          itemCount: items.length,
          separatorBuilder: (_, _) => SizedBox(width: d.gap),
          itemBuilder: (context, i) {
            final it = items[i];
            return PosterCard(
              item: it,
              width: width,
              onTap: it.id == current ? null : () => onOpen(it),
            );
          },
        ),
      ),
    );
  }
}

// --------------------------------------------------------------- tech info

String formatSize(AppLocalizations l, Locale locale, int bytes) {
  final tag = locale.toLanguageTag();
  if (bytes >= 1000 * 1000 * 1000) {
    return l.detailSizeGb(NumberFormat('0.0#', tag).format(bytes / 1e9));
  }
  return l.detailSizeMb(NumberFormat('0', tag).format(bytes / 1e6));
}

/// `HEVC Main 10 · 3840×1600 · 10-bit · 23.976 fps · Dolby Vision, HDR10`.
String videoLine(AppLocalizations l, Locale locale, TechInfo t) {
  final parts = <String>[
    if (t.videoCodec case final c?)
      [c.toUpperCase(), ?t.videoProfile].join(' '),
    if (t.width != null && t.height != null) '${t.width}×${t.height}',
    if (t.bitDepth case final b?) l.detailTechBitDepth(b),
    if (t.frameRate case final f?)
      l.detailTechFps(NumberFormat('0.###', locale.toLanguageTag()).format(f)),
    if (t.hdr.isNotEmpty) t.hdr.map(hdrLabel).join(', '),
  ];
  return parts.join('  ·  ');
}

String _languages(AppLocalizations l, List<String> codes) => codes.isEmpty
    ? l.detailTechNone
    : codes.map((c) => c.toUpperCase()).join(', ');

/// Container, size, duration, video, audio and subtitle tracks of the file
/// the item plays, and its path.
class TechInfoSection extends StatelessWidget {
  const TechInfoSection({
    super.key,
    required this.item,
    this.tech,
    this.version,
    this.notProbedHint = false,
  });
  final LibraryItem item;
  final TechInfo? tech;
  final MediaVersion? version;

  /// Say that the file has not been probed yet (libraries that probe).
  final bool notProbedHint;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final locale = Localizations.localeOf(context);
    final theme = Theme.of(context);
    final t = tech;
    final rows = <(String, String)>[
      if (version != null && item.versionCount > 1)
        (l.detailTechVersion, version!.label),
      (l.detailTechFile, t?.filePath ?? version?.filePath ?? item.filePath),
      if (t?.container case final c?) (l.detailTechContainer, c),
      if ((t?.sizeBytes ?? version?.sizeBytes ?? 0) > 0)
        (
          l.detailTechSize,
          formatSize(l, locale, t?.sizeBytes ?? version!.sizeBytes),
        ),
      if (formatRuntime(l, t?.durationMs ?? item.durationMs) case final r
          when r.isNotEmpty)
        (l.detailTechDuration, r),
      if (t != null && videoLine(l, locale, t).isNotEmpty)
        (l.detailTechVideo, videoLine(l, locale, t)),
      if (t != null &&
          (t.audioFormats.isNotEmpty || t.audioLanguages.isNotEmpty))
        (
          l.detailTechAudio,
          [
            if (t.audioFormats.isNotEmpty)
              t.audioFormats
                  .map((a) => audioLabel(a) ?? a.toUpperCase())
                  .join(', '),
            if (t.audioLanguages.isNotEmpty) _languages(l, t.audioLanguages),
          ].join('  ·  '),
        ),
      if (t != null)
        (l.detailTechSubtitles, _languages(l, t.subtitleLanguages)),
    ];
    final labelW = d.isPhone ? 96.0 : 150.0;
    return Padding(
      key: const ValueKey('detail-tech'),
      padding: EdgeInsets.only(
        left: d.pageInsets.left,
        right: d.pageInsets.right,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final (k, v) in rows)
            Padding(
              padding: const EdgeInsets.only(bottom: 6),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SizedBox(
                    width: labelW,
                    child: Text(
                      k,
                      style: theme.textTheme.labelMedium?.copyWith(
                        color: KColors.textTertiary,
                      ),
                    ),
                  ),
                  Expanded(
                    child: Text(
                      v,
                      style: theme.textTheme.bodyMedium?.copyWith(
                        color: KColors.textSecondary,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                  ),
                ],
              ),
            ),
          if (t == null && notProbedHint)
            Text(
              l.detailTechNotProbed,
              style: theme.textTheme.labelSmall?.copyWith(
                color: KColors.textTertiary,
              ),
            ),
        ],
      ),
    );
  }
}
