// SPDX-License-Identifier: MPL-2.0
//! The trust store: signed records of the devices of a family and of
//! revocations.
//!
//! Every device record is signed by the member that admitted it (its
//! *sponsor*); the founder's record is self-signed, and the family id is
//! derived from the founder's key, so any member can check the whole roster
//! on its own and rosters can be merged from any member (gossip) without a
//! server. A revocation is signed by any active member and names a key, not
//! only a device id: a device that pairs again gets a new record.
//!
//! The roster holds public keys only and is not secret; it is stored as a
//! JSON file next to the app data. Private keys stay in the
//! [`SecretStore`](pa_secrets::SecretStore).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::codec::{self, Transcript, hex_array};
use crate::identity::{self, DeviceIdentity, PublicKeys};
use crate::{Error, Result};

/// Longest device name in bytes.
pub const MAX_NAME_LEN: usize = 64;
/// Most records (devices plus revocations) in a roster.
pub const MAX_RECORDS: usize = 256;

/// What kind of device a member is (for icons and hub priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Phone,
    Tablet,
    Tv,
    Desktop,
    Server,
    /// A kind of a newer version.
    #[serde(other)]
    Other,
}

impl DeviceKind {
    /// Stable lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Tablet => "tablet",
            Self::Tv => "tv",
            Self::Desktop => "desktop",
            Self::Server => "server",
            Self::Other => "other",
        }
    }
}

/// A device of the family, signed by its sponsor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub device_id: String,
    /// Name shown in the device list ("Living room TV").
    pub name: String,
    pub kind: DeviceKind,
    pub keys: PublicKeys,
    /// When it was admitted, Unix ms of the sponsor's clock.
    pub added_ms: u64,
    /// Signing key of the member that admitted it; its own key for the
    /// founder.
    #[serde(with = "hex_array")]
    pub sponsor: [u8; 32],
    #[serde(with = "hex_array")]
    pub sig: [u8; 64],
}

impl Member {
    fn transcript(&self, family: &str) -> Transcript {
        let mut t = Transcript::new("kinolune.member.v1");
        t.put_str(family)
            .put_str(&self.device_id)
            .put_str(&self.name)
            .put_str(self.kind.as_str())
            .put(&self.keys.sign)
            .put(&self.keys.dh)
            .put_u64(self.added_ms)
            .put(&self.sponsor);
        t
    }

    fn check_fields(&self) -> Result<()> {
        if !codec::is_valid_device_id(&self.device_id) {
            return Err(Error::Invalid("device id"));
        }
        check_name(&self.name)
    }

    fn is_founder(&self) -> bool {
        self.sponsor == self.keys.sign
    }
}

/// Checks a device name: 1 to [`MAX_NAME_LEN`] bytes, no control characters.
pub fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > MAX_NAME_LEN || name.chars().any(char::is_control)
    {
        return Err(Error::Invalid("device name"));
    }
    Ok(())
}

/// A revoked key, signed by the member that revoked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revocation {
    /// Signing key of the revoked record.
    #[serde(with = "hex_array")]
    pub key: [u8; 32],
    /// Its device id (for display; the key is authoritative).
    pub device_id: String,
    pub revoked_ms: u64,
    /// Signing key of the member that revoked it.
    #[serde(with = "hex_array")]
    pub by: [u8; 32],
    #[serde(with = "hex_array")]
    pub sig: [u8; 64],
}

impl Revocation {
    fn transcript(&self, family: &str) -> Transcript {
        let mut t = Transcript::new("kinolune.revocation.v1");
        t.put_str(family)
            .put(&self.key)
            .put_str(&self.device_id)
            .put_u64(self.revoked_ms)
            .put(&self.by);
        t
    }
}

/// One row of the device list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub fingerprint: String,
    pub added_ms: u64,
    /// Set for revoked devices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_ms: Option<u64>,
}

/// What a [`Roster::merge`] took over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// New device records.
    pub members: usize,
    /// New revocations.
    pub revocations: usize,
    /// Records that did not verify or came from revoked members.
    pub rejected: usize,
}

impl MergeReport {
    /// Whether anything changed.
    pub fn changed(&self) -> bool {
        self.members + self.revocations > 0
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    v: u8,
    family: String,
    members: Vec<Member>,
    #[serde(default)]
    revoked: Vec<Revocation>,
}

/// The trust store of a family. Serializes as
/// `{"v":1,"family":…,"members":[…],"revoked":[…]}`; deserializing checks
/// every signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub struct Roster {
    family: String,
    members: BTreeMap<[u8; 32], Member>,
    revoked: BTreeMap<[u8; 32], Revocation>,
}

/// Family id of a founder key: 128 bits of its SHA-256, hex.
pub fn family_id_of(founder: &[u8; 32]) -> String {
    let mut t = Transcript::new("kinolune.family.v1");
    t.put(founder);
    codec::to_hex(&codec::sha256(t.bytes())[..16])
}

impl Roster {
    /// A new family with `founder` as its only device.
    pub fn found(founder: &DeviceIdentity, name: &str, kind: DeviceKind, now_ms: u64) -> Result<Self> {
        let keys = founder.public();
        let mut roster = Self {
            family: family_id_of(&keys.sign),
            members: BTreeMap::new(),
            revoked: BTreeMap::new(),
        };
        let member = roster.sign_member(founder, founder.device_id(), name, kind, keys, now_ms)?;
        roster.members.insert(keys.sign, member);
        Ok(roster)
    }

    fn sign_member(
        &self,
        sponsor: &DeviceIdentity,
        device_id: &str,
        name: &str,
        kind: DeviceKind,
        keys: PublicKeys,
        now_ms: u64,
    ) -> Result<Member> {
        let mut member = Member {
            device_id: device_id.to_owned(),
            name: name.to_owned(),
            kind,
            keys,
            added_ms: now_ms,
            sponsor: sponsor.public().sign,
            sig: [0; 64],
        };
        member.check_fields()?;
        member.sig = sponsor.sign(member.transcript(&self.family).bytes());
        Ok(member)
    }

    /// Family id (hex, 32 characters).
    pub fn family_id(&self) -> &str {
        &self.family
    }

    /// The record of signing key `key`, active or revoked.
    pub fn member(&self, key: &[u8; 32]) -> Option<&Member> {
        self.members.get(key)
    }

    /// The revocation of `key`, if revoked.
    pub fn revocation(&self, key: &[u8; 32]) -> Option<&Revocation> {
        self.revoked.get(key)
    }

    /// Whether `key` is a member and not revoked.
    pub fn is_active(&self, key: &[u8; 32]) -> bool {
        self.members.contains_key(key) && !self.revoked.contains_key(key)
    }

    /// Active device records.
    pub fn active(&self) -> impl Iterator<Item = &Member> {
        self.members
            .values()
            .filter(|m| !self.revoked.contains_key(&m.keys.sign))
    }

    /// Active records of `device_id` (normally one). Fails with
    /// [`Error::Revoked`] if it only has revoked records, [`Error::Unknown`]
    /// if none.
    pub fn authorize(&self, device_id: &str) -> Result<Vec<&Member>> {
        let records: Vec<&Member> = self
            .members
            .values()
            .filter(|m| m.device_id == device_id)
            .collect();
        if records.is_empty() {
            return Err(Error::Unknown(device_id.to_owned()));
        }
        let active: Vec<&Member> = records
            .into_iter()
            .filter(|m| !self.revoked.contains_key(&m.keys.sign))
            .collect();
        if active.is_empty() {
            return Err(Error::Revoked(device_id.to_owned()));
        }
        Ok(active)
    }

    /// The device list, newest first.
    pub fn devices(&self) -> Vec<Device> {
        let mut list: Vec<Device> = self
            .members
            .values()
            .map(|m| Device {
                device_id: m.device_id.clone(),
                name: m.name.clone(),
                kind: m.kind,
                fingerprint: m.keys.fingerprint(),
                added_ms: m.added_ms,
                revoked_ms: self.revoked.get(&m.keys.sign).map(|r| r.revoked_ms),
            })
            .collect();
        list.sort_by(|a, b| {
            b.added_ms
                .cmp(&a.added_ms)
                .then_with(|| a.device_id.cmp(&b.device_id))
        });
        list
    }

    fn active_sponsor(&self, sponsor: &DeviceIdentity) -> Result<()> {
        let key = sponsor.public().sign;
        if self.revoked.contains_key(&key) {
            return Err(Error::Revoked(sponsor.device_id().to_owned()));
        }
        if !self.members.contains_key(&key) {
            return Err(Error::Unknown(sponsor.device_id().to_owned()));
        }
        Ok(())
    }

    /// Admits a device on behalf of `sponsor` (an active member). Older
    /// active records of the same device id are revoked: the device got new
    /// keys. Admitting the same keys again returns the existing record.
    pub fn admit(
        &mut self,
        sponsor: &DeviceIdentity,
        device_id: &str,
        name: &str,
        kind: DeviceKind,
        keys: PublicKeys,
        now_ms: u64,
    ) -> Result<Member> {
        self.active_sponsor(sponsor)?;
        if self.revoked.contains_key(&keys.sign) {
            return Err(Error::Revoked(device_id.to_owned()));
        }
        if let Some(existing) = self.members.get(&keys.sign) {
            return if existing.device_id == device_id {
                Ok(existing.clone())
            } else {
                Err(Error::Invalid("key of another device"))
            };
        }
        let member = self.sign_member(sponsor, device_id, name, kind, keys, now_ms)?;
        if self.records() >= MAX_RECORDS {
            return Err(Error::Invalid("too many devices"));
        }
        let stale: Vec<[u8; 32]> = self
            .active()
            .filter(|m| m.device_id == device_id)
            .map(|m| m.keys.sign)
            .collect();
        for key in stale {
            self.revoke_key(sponsor, &key, now_ms)?;
        }
        self.members.insert(keys.sign, member.clone());
        Ok(member)
    }

    /// Revokes every active record of `device_id` on behalf of `by`.
    pub fn revoke(
        &mut self,
        by: &DeviceIdentity,
        device_id: &str,
        now_ms: u64,
    ) -> Result<Vec<Revocation>> {
        let keys: Vec<[u8; 32]> = self
            .authorize(device_id)?
            .into_iter()
            .map(|m| m.keys.sign)
            .collect();
        // A device may remove itself; otherwise the revoker must be active.
        if !keys.contains(&by.public().sign) {
            self.active_sponsor(by)?;
        }
        keys.iter()
            .map(|key| self.revoke_key(by, key, now_ms))
            .collect()
    }

    fn revoke_key(&mut self, by: &DeviceIdentity, key: &[u8; 32], now_ms: u64) -> Result<Revocation> {
        let member = self
            .members
            .get(key)
            .ok_or_else(|| Error::Unknown(codec::to_hex(key)))?;
        let mut rev = Revocation {
            key: *key,
            device_id: member.device_id.clone(),
            revoked_ms: now_ms,
            by: by.public().sign,
            sig: [0; 64],
        };
        rev.sig = by.sign(rev.transcript(&self.family).bytes());
        self.revoked.insert(*key, rev.clone());
        Ok(rev)
    }

    fn records(&self) -> usize {
        self.members.len() + self.revoked.len()
    }

    /// Adds a verified member record. `strict`: its sponsor must not be
    /// revoked (records from others); otherwise only known (own file).
    fn add_member(&mut self, m: &Member, strict: bool) -> Result<bool> {
        m.check_fields()?;
        if self.members.contains_key(&m.keys.sign) {
            return Ok(false);
        }
        if self.records() >= MAX_RECORDS {
            return Err(Error::Invalid("too many devices"));
        }
        let sponsor = if m.is_founder() {
            if family_id_of(&m.keys.sign) != self.family {
                return Err(Error::OtherFamily);
            }
            m.keys
        } else {
            let sponsor = self
                .members
                .get(&m.sponsor)
                .ok_or_else(|| Error::Unknown(codec::to_hex(&m.sponsor)))?;
            if strict && self.revoked.contains_key(&m.sponsor) {
                return Err(Error::Revoked(sponsor.device_id.clone()));
            }
            sponsor.keys
        };
        sponsor.verify(m.transcript(&self.family).bytes(), &m.sig, "member")?;
        self.members.insert(m.keys.sign, m.clone());
        Ok(true)
    }

    fn add_revocation(&mut self, r: &Revocation, strict: bool) -> Result<bool> {
        if self.revoked.contains_key(&r.key) {
            return Ok(false);
        }
        let member = self
            .members
            .get(&r.key)
            .ok_or_else(|| Error::Unknown(r.device_id.clone()))?;
        if member.device_id != r.device_id {
            return Err(Error::Invalid("revocation device id"));
        }
        let by = self
            .members
            .get(&r.by)
            .ok_or_else(|| Error::Unknown(codec::to_hex(&r.by)))?;
        if strict && r.by != r.key && self.revoked.contains_key(&r.by) {
            return Err(Error::Revoked(by.device_id.clone()));
        }
        identity::verify(&r.by, r.transcript(&self.family).bytes(), &r.sig, "revocation")?;
        if self.records() >= MAX_RECORDS {
            return Err(Error::Invalid("too many devices"));
        }
        self.revoked.insert(r.key, r.clone());
        Ok(true)
    }

    /// Takes over records of `other` (the same family) that verify: device
    /// records sponsored by active members and revocations by active
    /// members. Revocations always win; nothing is ever un-revoked.
    pub fn merge(&mut self, other: &Roster) -> Result<MergeReport> {
        if other.family != self.family {
            return Err(Error::OtherFamily);
        }
        let mut report = MergeReport::default();
        // Revocations of known keys first, so that a sponsor revoked in the
        // same update cannot bring in new devices.
        self.merge_revocations(other, &mut report);
        let mut pending: Vec<&Member> = other
            .members
            .values()
            .filter(|m| !self.members.contains_key(&m.keys.sign))
            .collect();
        loop {
            let before = pending.len();
            let mut rest = Vec::new();
            for m in pending {
                match self.add_member(m, true) {
                    Ok(true) => report.members += 1,
                    Ok(false) => {}
                    Err(_) => rest.push(m),
                }
            }
            pending = rest;
            if pending.len() == before {
                break;
            }
        }
        report.rejected += pending.len();
        self.merge_revocations(other, &mut report);
        Ok(report)
    }

    fn merge_revocations(&mut self, other: &Roster, report: &mut MergeReport) {
        let mut revs: Vec<&Revocation> = other
            .revoked
            .values()
            .filter(|r| !self.revoked.contains_key(&r.key) && self.members.contains_key(&r.key))
            .collect();
        revs.sort_by_key(|r| r.revoked_ms);
        for r in revs {
            match self.add_revocation(r, true) {
                Ok(true) => report.revocations += 1,
                Ok(false) => {}
                Err(_) => report.rejected += 1,
            }
        }
    }

    /// JSON text of the roster.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("roster serializes")
    }

    /// Parses and verifies a roster.
    pub fn from_json(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }

    /// The roster saved at `path`, `None` if there is no file.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        match fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Saves the roster to `path` atomically (temporary file and rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(self.to_json().as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

impl TryFrom<Wire> for Roster {
    type Error = Error;

    fn try_from(w: Wire) -> Result<Self> {
        if w.v != 1 {
            return Err(Error::Invalid("roster version"));
        }
        if w.family.len() != 32 || codec::from_hex(&w.family, 16).is_err() {
            return Err(Error::Invalid("family id"));
        }
        if w.members.len() + w.revoked.len() > MAX_RECORDS {
            return Err(Error::Invalid("too many devices"));
        }
        let mut roster = Self {
            family: w.family,
            members: BTreeMap::new(),
            revoked: BTreeMap::new(),
        };
        // Sponsors may come after the devices they admitted: repeat until
        // nothing more verifies.
        let mut pending: Vec<&Member> = w.members.iter().collect();
        loop {
            let before = pending.len();
            pending.retain(|m| roster.add_member(m, false).is_err());
            if pending.len() == before {
                break;
            }
        }
        if !pending.is_empty() {
            return Err(Error::BadSignature("member chain"));
        }
        if !roster.members.values().any(Member::is_founder) {
            return Err(Error::Invalid("roster without founder"));
        }
        for r in &w.revoked {
            roster.add_revocation(r, false)?;
        }
        Ok(roster)
    }
}

impl From<Roster> for Wire {
    fn from(r: Roster) -> Self {
        // Sponsors before the devices they admitted, so a reader verifies in
        // one pass.
        let mut members: Vec<Member> = r.members.into_values().collect();
        members.sort_by(|a, b| {
            b.is_founder()
                .cmp(&a.is_founder())
                .then(a.added_ms.cmp(&b.added_ms))
                .then_with(|| a.keys.sign.cmp(&b.keys.sign))
        });
        let mut revoked: Vec<Revocation> = r.revoked.into_values().collect();
        revoked.sort_by_key(|r| r.revoked_ms);
        Self {
            v: 1,
            family: r.family,
            members,
            revoked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> DeviceIdentity {
        DeviceIdentity::generate(name).unwrap()
    }

    #[test]
    fn founder_admits_and_revokes() {
        let phone = id("phone");
        let tv = id("tv");
        let mut roster = Roster::found(&phone, "Phone", DeviceKind::Phone, 1).unwrap();
        assert_eq!(roster.family_id().len(), 32);
        roster
            .admit(&phone, "tv", "TV", DeviceKind::Tv, tv.public(), 2)
            .unwrap();
        assert_eq!(roster.authorize("tv").unwrap().len(), 1);
        assert!(matches!(roster.authorize("pc"), Err(Error::Unknown(_))));

        let back = Roster::from_json(&roster.to_json()).unwrap();
        assert_eq!(back, roster);

        roster.revoke(&phone, "tv", 3).unwrap();
        assert!(matches!(roster.authorize("tv"), Err(Error::Revoked(_))));
        // A revoked device cannot admit others or come back with old keys.
        let pc = id("pc");
        assert!(roster
            .admit(&tv, "pc", "PC", DeviceKind::Desktop, pc.public(), 4)
            .is_err());
        assert!(matches!(
            roster.admit(&phone, "tv", "TV", DeviceKind::Tv, tv.public(), 5),
            Err(Error::Revoked(_))
        ));
        // With new keys it can pair again.
        let tv2 = id("tv");
        roster
            .admit(&phone, "tv", "TV", DeviceKind::Tv, tv2.public(), 6)
            .unwrap();
        assert_eq!(roster.authorize("tv").unwrap()[0].keys, tv2.public());
        let list = roster.devices();
        assert_eq!(list.len(), 3);
        assert_eq!(list.iter().filter(|d| d.revoked_ms.is_some()).count(), 1);
        assert_eq!(Roster::from_json(&roster.to_json()).unwrap(), roster);
    }

    #[test]
    fn tampered_rosters_are_rejected() {
        let phone = id("phone");
        let tv = id("tv");
        let mut roster = Roster::found(&phone, "Phone", DeviceKind::Phone, 1).unwrap();
        roster
            .admit(&phone, "tv", "TV", DeviceKind::Tv, tv.public(), 2)
            .unwrap();
        let json = roster.to_json();
        assert!(Roster::from_json(&json.replace("\"TV\"", "\"Tv2\"")).is_err());
        // A self-signed device that is not the founder.
        let evil = id("evil");
        let other = Roster::found(&evil, "Evil", DeviceKind::Phone, 1).unwrap();
        let mut wire: serde_json::Value = serde_json::from_str(&json).unwrap();
        let evil_member: serde_json::Value =
            serde_json::from_str::<serde_json::Value>(&other.to_json()).unwrap()["members"][0]
                .clone();
        wire["members"].as_array_mut().unwrap().push(evil_member);
        assert!(Roster::from_json(&wire.to_string()).is_err());
        assert!(matches!(roster.merge(&other), Err(Error::OtherFamily)));
    }

    #[test]
    fn merge_spreads_admissions_and_revocations() {
        let phone = id("phone");
        let tv = id("tv");
        let pc = id("pc");
        let mut a = Roster::found(&phone, "Phone", DeviceKind::Phone, 1).unwrap();
        a.admit(&phone, "tv", "TV", DeviceKind::Tv, tv.public(), 2)
            .unwrap();
        let mut b = a.clone();
        // The TV admits the PC; the phone learns it by merge.
        b.admit(&tv, "pc", "PC", DeviceKind::Desktop, pc.public(), 3)
            .unwrap();
        let report = a.merge(&b).unwrap();
        assert_eq!(report.members, 1);
        assert!(a.authorize("pc").is_ok());
        // The phone revokes the TV; on merge B also drops it, and devices
        // the TV admits afterwards are not taken over.
        a.revoke(&phone, "tv", 4).unwrap();
        let laptop = id("laptop");
        b.admit(&tv, "laptop", "Laptop", DeviceKind::Desktop, laptop.public(), 5)
            .unwrap();
        let report = a.merge(&b).unwrap();
        assert_eq!(report.members, 0);
        assert_eq!(report.rejected, 1);
        let report = b.merge(&a).unwrap();
        assert_eq!(report.revocations, 1);
        assert!(matches!(b.authorize("tv"), Err(Error::Revoked(_))));
        // Merging again changes nothing.
        assert!(!b.merge(&a).unwrap().changed());
    }
}
