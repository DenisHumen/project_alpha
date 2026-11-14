-- SPDX-License-Identifier: MPL-2.0
--
-- UI-009: cast, crew and external ratings of a work for the Detail screen.
-- Expand-only. The metadata run (META-013) rewrites a provider's rows of a
-- work on every fetch.
--
-- work_credits   people of a work by provider, in display order: `cast`
--                (actors in billing order, `character` is the role) and
--                `crew` (`job`: Director, Screenplay…). `person_ref` is the
--                provider's person id for people pages (UI-009 P1);
--                `profile_url` an https URL of the provider's CDN.
-- work_ratings   average user rating of a work, 0–10, by source (`tmdb`,
--                later `imdb`…), with the number of votes.

CREATE TABLE work_credits (
    work_id     INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    provider    TEXT    NOT NULL CHECK (provider <> '' AND provider NOT GLOB '*[^a-z0-9_]*'),
    role        TEXT    NOT NULL CHECK (role IN ('cast', 'crew')),
    position    INTEGER NOT NULL CHECK (position >= 0),
    person_ref  TEXT,
    name        TEXT    NOT NULL CHECK (name <> ''),
    character   TEXT,
    job         TEXT,
    profile_url TEXT,
    PRIMARY KEY (work_id, provider, role, position)
) STRICT;

CREATE TABLE work_ratings (
    work_id  INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    provider TEXT    NOT NULL CHECK (provider <> '' AND provider NOT GLOB '*[^a-z0-9_]*'),
    value    REAL    NOT NULL CHECK (value >= 0 AND value <= 10),
    votes    INTEGER CHECK (votes >= 0),
    PRIMARY KEY (work_id, provider)
) STRICT;
