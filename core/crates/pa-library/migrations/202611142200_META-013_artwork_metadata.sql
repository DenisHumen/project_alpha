-- SPDX-License-Identifier: MPL-2.0
--
-- META-013: provider metadata in the library — artwork, episode overviews
-- and the per-work fetch state. Expand-only.
--
-- artwork        images of a work (poster, backdrop, logo) or an episode
--                (still) by provider; the app shows the row with the
--                lowest `priority` of a kind. `url` is an https URL of the
--                provider's CDN (hotlinked, not downloaded) or a file:// URI.
-- work_metadata  last metadata fetch of a work: outcome, language and time,
--                so a scan does not ask providers again for every work and
--                provider data is refreshed well within TMDB's six-month
--                cache limit.

ALTER TABLE episodes ADD COLUMN overview TEXT;

CREATE TABLE artwork (
    id         INTEGER PRIMARY KEY,
    work_id    INTEGER REFERENCES works (id) ON DELETE CASCADE,
    episode_id INTEGER REFERENCES episodes (id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL CHECK (kind IN ('poster', 'backdrop', 'logo', 'still')),
    -- tmdb, anilist, shikimori, local…
    provider   TEXT    NOT NULL CHECK (provider <> '' AND provider NOT GLOB '*[^a-z0-9_]*'),
    url        TEXT    NOT NULL CHECK (url <> ''),
    -- ISO 639-1 of text on the image; '' if none or unknown.
    lang       TEXT    NOT NULL DEFAULT '',
    -- Lower is preferred.
    priority   INTEGER NOT NULL DEFAULT 0,
    CHECK ((work_id IS NULL) <> (episode_id IS NULL))
) STRICT;

CREATE UNIQUE INDEX artwork_work ON artwork (work_id, kind, provider) WHERE work_id IS NOT NULL;
CREATE UNIQUE INDEX artwork_episode ON artwork (episode_id, kind, provider)
    WHERE episode_id IS NOT NULL;

CREATE TABLE work_metadata (
    work_id       INTEGER PRIMARY KEY REFERENCES works (id) ON DELETE CASCADE,
    outcome       TEXT    NOT NULL CHECK (outcome IN ('matched', 'not_found', 'failed')),
    -- Metadata language of the fetch (`ru-RU`); a new language refetches.
    language      TEXT    NOT NULL DEFAULT '',
    fetched_at_ms INTEGER NOT NULL,
    error         TEXT
) STRICT;
