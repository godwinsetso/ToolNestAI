# READIT PRIVACY POLICY

READIT is designed so that your reading stays yours.

## WHAT STAYS ON YOUR DEVICE

- The documents you import (READIT keeps a private copy; your original file is never changed).
- Extracted text, reading positions, bookmarks, notes, listening history, collections, settings and pronunciation rules (a local SQLite database).
- Downloaded voices and any audio READIT generates.

Android cloud backup and device-to-device transfer are switched off for READIT, so none of this is copied to Google's servers.

## WHAT GOES ONLINE, AND WHEN

| Action | What is sent | Started by |
|---|---|---|
| Downloading a voice | A download request to GitHub for that voice file | You, by tapping Download |
| Using a phone voice marked "Needs internet" | The text being read, sent by your phone's TTS app under that app's own policy | You, by selecting that voice; it is labelled |

Nothing else. READIT has no account, no analytics, no advertising, no crash reporting and no server. It never uploads documents, recordings or reading history.

## DELETION

- **Remove a document:** deletes READIT's copy, its bookmarks, notes and generated audio.
- **Remove a voice:** deletes its files.
- **Clear generated audio:** deletes cached speech only.
- **Settings > Erase all READIT data:** deletes everything above, and the database, from this device.
- Uninstalling READIT removes all of its data.

## SECURITY MEASURES

- Files are parsed by READIT's own extractors; macros, scripts and embedded objects are never executed. HTML scripts and EPUB code are ignored.
- Archive contents are size-limited and checked for path traversal; voice downloads are verified by SHA-256 before use and only follow HTTPS redirects.
- Search input cannot alter database queries.
- Data sits in Android's private app storage, protected by the app sandbox and the device's file-based encryption. Additional database encryption is planned.

# VOICE CONSENT WORKFLOW (MY VOICE, PHASE 3)

Personal voices are not available in Phase 1. The rules below are already enforced by READIT's core library (and covered by automated tests), so the feature will be built on them.

1. **Before recording**, READIT states where processing will happen (on this device, or a named external service) and asks for confirmation.
2. **Consent record.** A personal voice can only be created against a consent record that names the person and states the relationship: *self*, or *another person who has authorised it*. A record with any other relationship, or with no name or statement, is rejected.
3. **Another person's voice** requires the user to confirm that person's permission; the confirmation text is stored with the record.
4. **Labelling.** Synthetic voices are always labelled as synthetic and are never presented as a genuine recording.
5. **Revocation.** Revoking consent permanently disables every voice built on it and deletes the recordings and model files READIT holds. A revoked record cannot be reused.
6. **No training on users' voices.** Recordings are never used to train shared models.
7. **No substitution.** If a device cannot run personal voice creation, READIT says so; it never presents a generic voice as the user's own.
