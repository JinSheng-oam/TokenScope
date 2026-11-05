package sqlstore

import (
	"context"
	"database/sql"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"example.com/tokenscope/hub/internal/hub"
)

//go:embed migrations/001.sql
var migration001 string

//go:embed migrations/002.sql
var migration002 string

//go:embed migrations/003.sql
var migration003 string

//go:embed migrations/004.sql
var migration004 string

const schemaVersion = 4

type SQLiteRepository struct{ db *sql.DB }

var _ hub.Repository = (*SQLiteRepository)(nil)

func Open(path string) (*SQLiteRepository, error) {
	if strings.TrimSpace(path) == "" {
		return nil, errors.New("database path is required")
	}
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, fmt.Errorf("create database directory: %w", err)
	}
	if runtime.GOOS != "windows" {
		info, err := os.Stat(dir)
		if err != nil {
			return nil, fmt.Errorf("stat database directory: %w", err)
		}
		if info.Mode().Perm()&0o077 != 0 {
			return nil, errors.New("Hub database directory must be private (mode 0700 or stricter)")
		}
	}
	db, err := sql.Open("sqlite", path)
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	for _, pragma := range []string{"PRAGMA foreign_keys=ON", "PRAGMA busy_timeout=5000", "PRAGMA journal_mode=WAL", "PRAGMA synchronous=FULL"} {
		if _, err := db.Exec(pragma); err != nil {
			_ = db.Close()
			return nil, fmt.Errorf("sqlite configuration: %w", err)
		}
	}
	if err := migrate(context.Background(), db); err != nil {
		_ = db.Close()
		return nil, err
	}
	if runtime.GOOS != "windows" {
		if err := os.Chmod(path, 0o600); err != nil {
			_ = db.Close()
			return nil, fmt.Errorf("restrict database permissions: %w", err)
		}
	}
	return &SQLiteRepository{db: db}, nil
}

func migrate(ctx context.Context, db *sql.DB) error {
	var current int
	if err := db.QueryRowContext(ctx, "PRAGMA user_version").Scan(&current); err != nil {
		return err
	}
	if current > schemaVersion {
		return fmt.Errorf("unsupported hub schema version %d", current)
	}
	if current == 0 {
		var existing int
		if err := db.QueryRowContext(ctx, `SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('devices','device_credentials','pairing_codes','schema_migrations')`).Scan(&existing); err != nil {
			return err
		}
		if existing != 0 {
			return errors.New("unversioned Hub database contains known tables; refusing implicit adoption")
		}
	}
	if current > 0 {
		var count, highest int
		if err := db.QueryRowContext(ctx, `SELECT COUNT(*) FROM schema_migrations WHERE version=?`, current).Scan(&count); err != nil {
			return fmt.Errorf("hub migration metadata missing: %w", err)
		}
		if err := db.QueryRowContext(ctx, `SELECT COALESCE(MAX(version),0) FROM schema_migrations`).Scan(&highest); err != nil {
			return err
		}
		if count != 1 || highest != current {
			return errors.New("hub schema_migrations disagrees with user_version")
		}
	}
	if current == schemaVersion {
		var count, highest int
		if err := db.QueryRowContext(ctx, `SELECT COUNT(*) FROM schema_migrations WHERE version=?`, schemaVersion).Scan(&count); err != nil {
			return fmt.Errorf("hub migration metadata missing: %w", err)
		}
		if err := db.QueryRowContext(ctx, `SELECT COALESCE(MAX(version),0) FROM schema_migrations`).Scan(&highest); err != nil {
			return err
		}
		if count != 1 || highest != schemaVersion {
			return errors.New("hub schema_migrations disagrees with user_version")
		}
		return nil
	}
	tx, err := db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	migrations := []struct {
		version int
		sql     string
	}{}
	if current < 1 {
		migrations = append(migrations, struct {
			version int
			sql     string
		}{1, migration001})
	}
	if current < 2 {
		migrations = append(migrations, struct {
			version int
			sql     string
		}{2, migration002})
	}
	if current < 3 {
		migrations = append(migrations, struct {
			version int
			sql     string
		}{3, migration003})
	}
	if current < 4 {
		migrations = append(migrations, struct {
			version int
			sql     string
		}{4, migration004})
	}
	for _, m := range migrations {
		for _, statement := range strings.Split(m.sql, ";") {
			statement = strings.TrimSpace(statement)
			if statement == "" {
				continue
			}
			if _, err = tx.ExecContext(ctx, statement); err != nil {
				return fmt.Errorf("apply hub migration %03d: %w", m.version, err)
			}
		}
		if _, err = tx.ExecContext(ctx, "INSERT INTO schema_migrations(version,applied_at_ms) VALUES(?,CAST(strftime('%s','now') AS INTEGER)*1000)", m.version); err != nil {
			return err
		}
		if _, err = tx.ExecContext(ctx, fmt.Sprintf("PRAGMA user_version=%d", m.version)); err != nil {
			return err
		}
	}
	return tx.Commit()
}

func (s *SQLiteRepository) CreatePairingCode(ctx context.Context, codeHash string, createdAtMS, expiresAtMS int64) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	// Retain consumed/expired code hashes briefly for operational diagnosis,
	// then prune them. Raw codes are never persisted.
	if _, err := tx.ExecContext(ctx, `DELETE FROM pairing_codes WHERE expires_at_ms < ?`, createdAtMS-7*24*60*60*1000); err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO pairing_codes(code_sha256,created_at_ms,expires_at_ms) VALUES(?,?,?)`, codeHash, createdAtMS, expiresAtMS)
	if isUnique(err) {
		return hub.ErrCredentialConflict
	}
	if err != nil {
		return err
	}
	return tx.Commit()
}

func (s *SQLiteRepository) PairDevice(ctx context.Context, codeHash string, nowMS int64, d hub.Device, credentialHash string) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	res, err := tx.ExecContext(ctx, `UPDATE pairing_codes SET consumed_at_ms=? WHERE code_sha256=? AND consumed_at_ms IS NULL AND expires_at_ms>?`, nowMS, codeHash, nowMS)
	if err != nil {
		return err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n != 1 {
		return hub.ErrInvalidPairingCode
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO devices(device_id,display_name,os_name,app_version,time_zone,created_at_ms) VALUES(?,?,?,?,?,?)`, d.DeviceID, d.DisplayName, d.OSName, d.AppVersion, d.TimeZone, d.CreatedAtMS)
	if isUnique(err) {
		return hub.ErrDeviceExists
	}
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO device_credentials(device_id,token_sha256,created_at_ms) VALUES(?,?,?)`, d.DeviceID, credentialHash, nowMS)
	if isUnique(err) {
		return hub.ErrCredentialConflict
	}
	if err != nil {
		return err
	}
	return tx.Commit()
}

func (s *SQLiteRepository) Authenticate(ctx context.Context, credentialHash string, nowMS int64) (hub.Device, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return hub.Device{}, err
	}
	defer tx.Rollback()
	var d hub.Device
	var lastSeen, revoked, credentialRevoked sql.NullInt64
	err = tx.QueryRowContext(ctx, `SELECT d.device_id,d.display_name,d.os_name,d.app_version,d.time_zone,d.created_at_ms,d.last_seen_ms,d.revoked_at_ms,c.revoked_at_ms FROM device_credentials c JOIN devices d ON d.device_id=c.device_id WHERE c.token_sha256=?`, credentialHash).
		Scan(&d.DeviceID, &d.DisplayName, &d.OSName, &d.AppVersion, &d.TimeZone, &d.CreatedAtMS, &lastSeen, &revoked, &credentialRevoked)
	if errors.Is(err, sql.ErrNoRows) {
		return hub.Device{}, hub.ErrInvalidCredential
	}
	if err != nil {
		return hub.Device{}, err
	}
	d.LastSeenMS = nullInt(lastSeen)
	d.RevokedAtMS = nullInt(revoked)
	if d.Revoked() {
		return hub.Device{}, hub.ErrDeviceRevoked
	}
	if credentialRevoked.Valid {
		return hub.Device{}, hub.ErrInvalidCredential
	}
	if _, err := tx.ExecContext(ctx, `UPDATE devices SET last_seen_ms=? WHERE device_id=? AND revoked_at_ms IS NULL`, nowMS, d.DeviceID); err != nil {
		return hub.Device{}, err
	}
	d.LastSeenMS = &nowMS
	if err := tx.Commit(); err != nil {
		return hub.Device{}, err
	}
	return d, nil
}

func (s *SQLiteRepository) ListDevices(ctx context.Context) ([]hub.Device, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT device_id,display_name,os_name,app_version,time_zone,created_at_ms,last_seen_ms,revoked_at_ms FROM devices ORDER BY created_at_ms,device_id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make([]hub.Device, 0)
	for rows.Next() {
		var d hub.Device
		var last, revoked sql.NullInt64
		if err := rows.Scan(&d.DeviceID, &d.DisplayName, &d.OSName, &d.AppVersion, &d.TimeZone, &d.CreatedAtMS, &last, &revoked); err != nil {
			return nil, err
		}
		d.LastSeenMS = nullInt(last)
		d.RevokedAtMS = nullInt(revoked)
		out = append(out, d)
	}
	return out, rows.Err()
}

func (s *SQLiteRepository) RenameDevice(ctx context.Context, id, name string) error {
	res, err := s.db.ExecContext(ctx, `UPDATE devices SET display_name=? WHERE device_id=? AND revoked_at_ms IS NULL`, name, id)
	if err != nil {
		return err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n == 1 {
		return nil
	}
	return s.deviceStateError(ctx, id)
}

func (s *SQLiteRepository) RotateCredential(ctx context.Context, id, oldHash, newHash string, nowMS int64) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	res, err := tx.ExecContext(ctx, `UPDATE device_credentials SET revoked_at_ms=? WHERE device_id=? AND token_sha256=? AND revoked_at_ms IS NULL AND EXISTS(SELECT 1 FROM devices WHERE device_id=? AND revoked_at_ms IS NULL)`, nowMS, id, oldHash, id)
	if err != nil {
		return err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n != 1 {
		var deviceRevoked, credentialRevoked sql.NullInt64
		stateErr := tx.QueryRowContext(ctx, `SELECT d.revoked_at_ms,c.revoked_at_ms FROM devices d LEFT JOIN device_credentials c ON c.device_id=d.device_id AND c.token_sha256=? WHERE d.device_id=?`, oldHash, id).Scan(&deviceRevoked, &credentialRevoked)
		if errors.Is(stateErr, sql.ErrNoRows) {
			return hub.ErrDeviceNotFound
		}
		if stateErr != nil {
			return stateErr
		}
		if deviceRevoked.Valid {
			return hub.ErrDeviceRevoked
		}
		return hub.ErrInvalidCredential
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO device_credentials(device_id,token_sha256,created_at_ms) VALUES(?,?,?)`, id, newHash, nowMS)
	if isUnique(err) {
		return hub.ErrCredentialConflict
	}
	if err != nil {
		return err
	}
	return tx.Commit()
}

func (s *SQLiteRepository) RevokeDevice(ctx context.Context, id string, nowMS int64) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	res, err := tx.ExecContext(ctx, `UPDATE devices SET revoked_at_ms=COALESCE(revoked_at_ms,?) WHERE device_id=?`, nowMS, id)
	if err != nil {
		return err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n == 0 {
		return hub.ErrDeviceNotFound
	}
	if _, err := tx.ExecContext(ctx, `UPDATE device_credentials SET revoked_at_ms=COALESCE(revoked_at_ms,?) WHERE device_id=?`, nowMS, id); err != nil {
		return err
	}
	return tx.Commit()
}

func (s *SQLiteRepository) deviceStateError(ctx context.Context, id string) error {
	var revoked sql.NullInt64
	err := s.db.QueryRowContext(ctx, `SELECT revoked_at_ms FROM devices WHERE device_id=?`, id).Scan(&revoked)
	if errors.Is(err, sql.ErrNoRows) {
		return hub.ErrDeviceNotFound
	}
	if err != nil {
		return err
	}
	if revoked.Valid {
		return hub.ErrDeviceRevoked
	}
	return hub.ErrDeviceNotFound
}
func (s *SQLiteRepository) credentialStateError(ctx context.Context, id, hash string) error {
	var deviceRevoked, credentialRevoked sql.NullInt64
	err := s.db.QueryRowContext(ctx, `SELECT d.revoked_at_ms,c.revoked_at_ms FROM devices d LEFT JOIN device_credentials c ON c.device_id=d.device_id AND c.token_sha256=? WHERE d.device_id=?`, hash, id).Scan(&deviceRevoked, &credentialRevoked)
	if errors.Is(err, sql.ErrNoRows) {
		return hub.ErrDeviceNotFound
	}
	if err != nil {
		return err
	}
	if deviceRevoked.Valid {
		return hub.ErrDeviceRevoked
	}
	return hub.ErrInvalidCredential
}
func nullInt(v sql.NullInt64) *int64 {
	if !v.Valid {
		return nil
	}
	n := v.Int64
	return &n
}
func isUnique(err error) bool {
	return err != nil && strings.Contains(strings.ToLower(err.Error()), "unique constraint")
}

func (s *SQLiteRepository) ApplyUsageBatch(ctx context.Context, deviceID string, records []hub.UsageEnvelope, nowMS int64) ([]hub.UsageItemResult, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()
	var revoked sql.NullInt64
	if err = tx.QueryRowContext(ctx, `SELECT revoked_at_ms FROM devices WHERE device_id=?`, deviceID).Scan(&revoked); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return nil, hub.ErrDeviceNotFound
		}
		return nil, err
	}
	if revoked.Valid {
		return nil, hub.ErrDeviceRevoked
	}
	results := make([]hub.UsageItemResult, 0, len(records))
	for _, e := range records {
		raw, marshalErr := json.Marshal(e.Record)
		if marshalErr != nil {
			return nil, marshalErr
		}
		var oldRev int64
		var oldHash string
		qerr := tx.QueryRowContext(ctx, `SELECT record_revision,payload_hash FROM usage_events WHERE device_id=? AND record_id=?`, deviceID, e.Record.RecordID).Scan(&oldRev, &oldHash)
		status := "accepted"
		hubRev := e.RecordRevision
		if qerr == nil {
			hubRev = uint64(oldRev)
			switch {
			case e.RecordRevision < uint64(oldRev):
				status = "superseded"
			case e.RecordRevision == uint64(oldRev):
				if e.PayloadHash != oldHash {
					return nil, hub.ErrRevisionConflict
				}
				status = "unchanged"
			default:
				status = "accepted"
				hubRev = e.RecordRevision
			}
		} else if !errors.Is(qerr, sql.ErrNoRows) {
			return nil, qerr
		}
		if status == "accepted" {
			_, err = tx.ExecContext(ctx, `INSERT INTO usage_events(device_id,record_id,record_revision,payload_hash,payload_json,received_at_ms) VALUES(?,?,?,?,?,?) ON CONFLICT(device_id,record_id) DO UPDATE SET record_revision=excluded.record_revision,payload_hash=excluded.payload_hash,payload_json=excluded.payload_json,received_at_ms=excluded.received_at_ms`, deviceID, e.Record.RecordID, e.RecordRevision, e.PayloadHash, string(raw), nowMS)
			if err != nil {
				return nil, err
			}
			_, err = tx.ExecContext(ctx, `INSERT INTO usage_change_feed(device_id,record_id,record_revision,payload_hash,payload_json,changed_at_ms) VALUES(?,?,?,?,?,?)`, deviceID, e.Record.RecordID, e.RecordRevision, e.PayloadHash, string(raw), nowMS)
			if err != nil {
				return nil, err
			}
		}
		results = append(results, hub.UsageItemResult{RecordID: e.Record.RecordID, RecordRevision: e.RecordRevision, Status: status, HubRevision: hubRev})
	}
	if err = tx.Commit(); err != nil {
		return nil, err
	}
	return results, nil
}

func (s *SQLiteRepository) PullUsageChanges(ctx context.Context, afterSeq int64, limit int) ([]hub.UsageChange, int64, error) {
	if afterSeq < 0 || limit < 1 || limit > 500 {
		return nil, 0, hub.ErrInvalidInput
	}
	var high int64
	if err := s.db.QueryRowContext(ctx, `SELECT COALESCE(MAX(change_seq),0) FROM usage_change_feed`).Scan(&high); err != nil {
		return nil, 0, err
	}
	rows, err := s.db.QueryContext(ctx, `SELECT change_seq,device_id,record_id,record_revision,payload_hash,payload_json,changed_at_ms FROM usage_change_feed WHERE change_seq>? AND change_seq<=? ORDER BY change_seq LIMIT ?`, afterSeq, high, limit)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()
	out := make([]hub.UsageChange, 0, limit)
	for rows.Next() {
		var seq, rev, changed int64
		var deviceID, recordID, hash, raw string
		if err := rows.Scan(&seq, &deviceID, &recordID, &rev, &hash, &raw, &changed); err != nil {
			return nil, 0, err
		}
		var record hub.UsageRecord
		if err := json.Unmarshal([]byte(raw), &record); err != nil {
			return nil, 0, fmt.Errorf("decode feed record: %w", err)
		}
		if record.RecordID != recordID || record.OwnerDeviceID != deviceID || rev <= 0 || changed < 0 || len(hash) != 64 {
			return nil, 0, errors.New("Hub change feed row failed integrity checks")
		}
		out = append(out, hub.UsageChange{ChangeSeq: seq, OwnerDeviceID: deviceID, Envelope: hub.UsageEnvelope{Record: record, RecordRevision: uint64(rev), PayloadHash: hash}, ChangedAtMS: changed})
	}
	if err := rows.Err(); err != nil {
		return nil, 0, err
	}
	return out, high, nil
}

func (s *SQLiteRepository) UpsertProjectNames(ctx context.Context, deviceID string, names []hub.ProjectName, nowMS int64) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	var revoked sql.NullInt64
	if err = tx.QueryRowContext(ctx, `SELECT revoked_at_ms FROM devices WHERE device_id=?`, deviceID).Scan(&revoked); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return hub.ErrDeviceNotFound
		}
		return err
	}
	if revoked.Valid {
		return hub.ErrDeviceRevoked
	}
	for _, name := range names {
		_, err = tx.ExecContext(ctx, `INSERT INTO project_names(device_id,project_id,name,updated_at_ms) VALUES(?,?,?,?) ON CONFLICT(device_id,project_id) DO UPDATE SET name=excluded.name,updated_at_ms=excluded.updated_at_ms WHERE project_names.name<>excluded.name`, deviceID, name.ProjectID, name.Name, nowMS)
		if err != nil {
			return err
		}
	}
	return tx.Commit()
}

func (s *SQLiteRepository) ListProjectNames(ctx context.Context, afterDeviceID, afterProjectID string, limit int) ([]hub.ProjectName, bool, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT device_id,project_id,name,updated_at_ms FROM project_names WHERE device_id>? OR (device_id=? AND project_id>?) ORDER BY device_id,project_id LIMIT ?`, afterDeviceID, afterDeviceID, afterProjectID, limit+1)
	if err != nil {
		return nil, false, err
	}
	defer rows.Close()
	names := make([]hub.ProjectName, 0, limit+1)
	for rows.Next() {
		var name hub.ProjectName
		if err := rows.Scan(&name.OwnerDeviceID, &name.ProjectID, &name.Name, &name.UpdatedAtMS); err != nil {
			return nil, false, err
		}
		names = append(names, name)
	}
	if err := rows.Err(); err != nil {
		return nil, false, err
	}
	hasMore := len(names) > limit
	if hasMore {
		names = names[:limit]
	}
	return names, hasMore, nil
}

func (s *SQLiteRepository) Close() error { return s.db.Close() }
