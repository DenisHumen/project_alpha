// SPDX-License-Identifier: MPL-2.0

// What the Detail screen shows besides the item row (UI-009): genres,
// clearlogo, ratings, external ids, cast and crew, extras and the probe
// data of the version the item plays. RustLibrary reads them from the
// core; the v0 library has none (the app then shows the v0 detail page).

import 'models.dart';

/// An average user rating, 0–10.
class ExternalRating {
  /// `tmdb`, …
  final String source;
  final double value;
  final int? votes;

  const ExternalRating({required this.source, required this.value, this.votes});
}

/// An id of the work at a provider (`imdb` `tt0133093`).
class ExternalId {
  final String provider;

  /// Entity type inside the provider (`movie`, `tv`); empty if none.
  final String kind;
  final String id;

  const ExternalId({required this.provider, required this.id, this.kind = ''});
}

/// A person of the cast ([character]) or crew ([job]).
class CreditPerson {
  final String name;
  final bool crew;
  final String? character;

  /// `Director`, `Screenplay`, … (as the provider writes it).
  final String? job;

  /// `tmdb:<person id>`, for people pages.
  final String? personRef;
  final String? photoUrl;

  const CreditPerson({
    required this.name,
    this.crew = false,
    this.character,
    this.job,
    this.personRef,
    this.photoUrl,
  });
}

/// Kind of an extra (LIB-007 names).
enum MediaExtraKind {
  trailer,
  teaser,
  featurette,
  behindTheScenes,
  deletedScene,
  interview,
  short,
  scene,
  other;

  static MediaExtraKind parse(String slug) => switch (slug) {
    'trailer' => trailer,
    'teaser' => teaser,
    'featurette' => featurette,
    'behind_the_scenes' => behindTheScenes,
    'deleted_scene' => deletedScene,
    'interview' => interview,
    'short' => short,
    'scene' => scene,
    _ => other,
  };
}

/// A trailer, featurette… next to the main file.
class MediaExtra {
  final String title;
  final MediaExtraKind kind;

  /// Display path ([LibraryItem.filePath]).
  final String filePath;

  /// Loopback stream URL of a network file ([LibraryItem.streamUri]).
  final String? streamUri;
  final int sizeBytes;
  final int? durationMs;

  const MediaExtra({
    required this.title,
    required this.kind,
    required this.filePath,
    this.streamUri,
    this.sizeBytes = 0,
    this.durationMs,
  });

  /// The URI to hand to the player.
  String get playbackUri => streamUri ?? filePath;
}

/// HDR format of a video stream.
enum HdrFormat { dolbyVision, hdr10Plus, hdr10, hlg }

/// What the probe found in the file an item plays.
class TechInfo {
  /// libavformat demuxer (`matroska,webm`, `mov,mp4,m4a,3gp,3g2,mj2`).
  final String? container;
  final int sizeBytes;
  final int? durationMs;

  /// FFmpeg codec name (`hevc`, `h264`, `av1`).
  final String? videoCodec;
  final String? videoProfile;
  final int? width;
  final int? height;
  final int? bitDepth;
  final double? frameRate;

  /// Strongest first.
  final List<HdrFormat> hdr;

  /// Audio format slugs in stream order (`truehd`, `atmos`, `eac3`,
  /// `dts_hd_ma`…), see pa-probe `audio_formats`.
  final List<String> audioFormats;

  /// ISO 639-2 codes.
  final List<String> audioLanguages;
  final List<String> subtitleLanguages;

  /// Display path of the file.
  final String? filePath;

  const TechInfo({
    this.container,
    this.sizeBytes = 0,
    this.durationMs,
    this.videoCodec,
    this.videoProfile,
    this.width,
    this.height,
    this.bitDepth,
    this.frameRate,
    this.hdr = const [],
    this.audioFormats = const [],
    this.audioLanguages = const [],
    this.subtitleLanguages = const [],
    this.filePath,
  });

  static HdrFormat? parseHdr(String slug) => switch (slug) {
    'dolby_vision' => HdrFormat.dolbyVision,
    'hdr10_plus' => HdrFormat.hdr10Plus,
    'hdr10' => HdrFormat.hdr10,
    'hlg' => HdrFormat.hlg,
    _ => null,
  };
}

/// Details of a movie or show for the Detail screen.
class ItemDetails {
  final List<String> genres;
  final int? runtimeMs;

  /// Clearlogo: a provider URL, a local `file://` URI or a stream URL.
  final String? logoUrl;
  final List<ExternalRating> ratings;
  final List<ExternalId> externalIds;

  /// Cast first (billing order), then crew.
  final List<CreditPerson> credits;

  /// A trailer URL from the provider (not fetched yet: always null).
  final String? trailerUrl;

  /// Trailers first.
  final List<MediaExtra> extras;
  final TechInfo? tech;

  const ItemDetails({
    this.genres = const [],
    this.runtimeMs,
    this.logoUrl,
    this.ratings = const [],
    this.externalIds = const [],
    this.credits = const [],
    this.trailerUrl,
    this.extras = const [],
    this.tech,
  });

  static const empty = ItemDetails();

  Iterable<CreditPerson> get cast => credits.where((c) => !c.crew);
  Iterable<CreditPerson> get crew => credits.where((c) => c.crew);
}

/// A library that knows the details of its items (UI-009).
abstract interface class LibraryDetails {
  /// Details of a movie or show item ([LibraryItem.id]); episodes get
  /// [ItemDetails.tech] only.
  Future<ItemDetails> details(String itemId);
}
