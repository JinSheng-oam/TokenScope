package sqlstore

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"example.com/tokenscope/hub/internal/hub"
	_ "modernc.org/sqlite"
)

func TestSQLitePairingLifecycleAndPersistence(t *testing.T) {
	ctx := context.Background()
	path := filepath.Join(t.TempDir(), "hub.db")
	repo, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	codeHash := strings.Repeat("a", 64)
	tokenHash := strings.Repeat("b", 64)
	rotatedHash := strings.Repeat("c", 64)
	if err := repo.CreatePairingCode(ctx, codeHash, 100, 400); err != nil {
		t.Fatal(err)
	}
	device := hub.Device{DeviceID: "office-pc-01", DisplayName: "Office PC", OSName: "windows", AppVersion: "0.1.0", TimeZone: "Asia/Shanghai", CreatedAtMS: 150}
	if err := repo.PairDevice(ctx, codeHash, 200, device, tokenHash); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.Authenticate(ctx, tokenHash, 250); err != nil {
		t.Fatal(err)
	}
	if err := repo.PairDevice(ctx, codeHash, 260, hub.Device{DeviceID: "office-pc-02", DisplayName: "Other PC", OSName: "linux", AppVersion: "0.1.0", TimeZone: "UTC", CreatedAtMS: 260}, strings.Repeat("d", 64)); !errors.Is(err, hub.ErrInvalidPairingCode) {
		t.Fatalf("pair replay: %v", err)
	}
	if err := repo.RotateCredential(ctx, device.DeviceID, tokenHash, rotatedHash, 300); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.Authenticate(ctx, tokenHash, 310); !errors.Is(err, hub.ErrInvalidCredential) {
		t.Fatalf("old token accepted: %v", err)
	}
	if _, err := repo.Authenticate(ctx, rotatedHash, 320); err != nil {
		t.Fatal(err)
	}
	if err := repo.RevokeDevice(ctx, device.DeviceID, 350); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.Authenticate(ctx, rotatedHash, 360); !errors.Is(err, hub.ErrDeviceRevoked) {
		t.Fatalf("revoked token accepted: %v", err)
	}
	devices, err := repo.ListDevices(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if len(devices) != 1 || devices[0].RevokedAtMS == nil {
		t.Fatalf("unexpected devices: %#v", devices)
	}
	if err := repo.Close(); err != nil {
		t.Fatal(err)
	}
	repo, err = Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	devices, err = repo.ListDevices(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if len(devices) != 1 || devices[0].DeviceID != "office-pc-01" {
		t.Fatalf("persistence failed: %#v", devices)
	}
}

func TestDuplicateDeviceRollsBackPairingCodeConsumption(t *testing.T) {
	ctx := context.Background()
	repo, err := Open(filepath.Join(t.TempDir(), "hub.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	firstCode := strings.Repeat("1", 64)
	secondCode := strings.Repeat("2", 64)
	if err := repo.CreatePairingCode(ctx, firstCode, 1, 1000); err != nil {
		t.Fatal(err)
	}
	if err := repo.CreatePairingCode(ctx, secondCode, 1, 1000); err != nil {
		t.Fatal(err)
	}
	d := hub.Device{DeviceID: "office-pc-01", DisplayName: "Office", OSName: "windows", AppVersion: "0.1", TimeZone: "UTC", CreatedAtMS: 10}
	if err := repo.PairDevice(ctx, firstCode, 10, d, strings.Repeat("a", 64)); err != nil {
		t.Fatal(err)
	}
	if err := repo.PairDevice(ctx, secondCode, 20, d, strings.Repeat("b", 64)); !errors.Is(err, hub.ErrDeviceExists) {
		t.Fatalf("expected duplicate device error, got %v", err)
	}
	d.DeviceID = "office-pc-02"
	if err := repo.PairDevice(ctx, secondCode, 30, d, strings.Repeat("c", 64)); err != nil {
		t.Fatalf("pairing code was consumed despite rollback: %v", err)
	}
}

func TestUsageBatchIdempotencyRevisionAndDeviceScope(t *testing.T) {
	ctx := context.Background()
	repo, err := Open(filepath.Join(t.TempDir(), "hub.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	code := strings.Repeat("a", 64)
	token := strings.Repeat("b", 64)
	device := hub.Device{DeviceID: "office-pc-01", DisplayName: "Office", OSName: "windows", AppVersion: "0.1.0", TimeZone: "UTC", CreatedAtMS: 100}
	if err := repo.CreatePairingCode(ctx, code, 100, 1000); err != nil {
		t.Fatal(err)
	}
	if err := repo.PairDevice(ctx, code, 110, device, token); err != nil {
		t.Fatal(err)
	}
	makeEnvelope := func(rev, output uint64) hub.UsageEnvelope {
		r := hub.UsageRecord{RecordID: strings.Repeat("a", 64), OwnerDeviceID: device.DeviceID, ToolID: "codex", Scope: "device_local", Precision: "exact", Usage: hub.UsageBreakdown{OutputTokens: output}}
		var b strings.Builder
		enc := json.NewEncoder(&b)
		enc.SetEscapeHTML(false)
		_ = enc.Encode(r)
		sum := sha256.Sum256(append([]byte("tokenscope/canonical-record/v1\x00"), []byte(strings.TrimSuffix(b.String(), "\n"))...))
		return hub.UsageEnvelope{Record: r, RecordRevision: rev, PayloadHash: hex.EncodeToString(sum[:])}
	}
	first := makeEnvelope(1, 10)
	got, err := repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{first}, 120)
	if err != nil {
		t.Fatal(err)
	}
	if got[0].Status != "accepted" {
		t.Fatal(got)
	}
	got, err = repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{first}, 130)
	if err != nil {
		t.Fatal(err)
	}
	if got[0].Status != "unchanged" {
		t.Fatal(got)
	}
	newer := makeEnvelope(2, 20)
	got, err = repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{newer}, 140)
	if err != nil {
		t.Fatal(err)
	}
	if got[0].HubRevision != 2 {
		t.Fatal(got)
	}
	got, err = repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{first}, 150)
	if err != nil {
		t.Fatal(err)
	}
	if got[0].Status != "superseded" || got[0].HubRevision != 2 {
		t.Fatal(got)
	}
	var revision int
	if err := repo.db.QueryRowContext(ctx, `SELECT record_revision FROM usage_events WHERE device_id=? AND record_id=?`, device.DeviceID, first.Record.RecordID).Scan(&revision); err != nil {
		t.Fatal(err)
	}
	if revision != 2 {
		t.Fatalf("revision=%d", revision)
	}
	conflict := makeEnvelope(2, 21)
	if _, err := repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{conflict}, 160); !errors.Is(err, hub.ErrRevisionConflict) {
		t.Fatalf("expected equal-revision conflict, got %v", err)
	}
	if err := repo.db.QueryRowContext(ctx, `SELECT record_revision FROM usage_events WHERE device_id=? AND record_id=?`, device.DeviceID, first.Record.RecordID).Scan(&revision); err != nil {
		t.Fatal(err)
	}
	if revision != 2 {
		t.Fatalf("conflict changed stored revision to %d", revision)
	}
}

func TestUsageChangeFeedPersistsEachAcceptedRevision(t *testing.T) {
	ctx := context.Background()
	repo, err := Open(filepath.Join(t.TempDir(), "hub.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	code := strings.Repeat("a", 64)
	token := strings.Repeat("b", 64)
	device := hub.Device{DeviceID: "office-pc-01", DisplayName: "Office", OSName: "windows", AppVersion: "0.1.0", TimeZone: "UTC", CreatedAtMS: 100}
	if err := repo.CreatePairingCode(ctx, code, 100, 1000); err != nil {
		t.Fatal(err)
	}
	if err := repo.PairDevice(ctx, code, 110, device, token); err != nil {
		t.Fatal(err)
	}
	makeEnv := func(rev, out uint64) hub.UsageEnvelope {
		r := hub.UsageRecord{RecordID: strings.Repeat("a", 64), OwnerDeviceID: device.DeviceID, ToolID: "codex", Scope: "device_local", Precision: "exact", Usage: hub.UsageBreakdown{OutputTokens: out}}
		var b strings.Builder
		enc := json.NewEncoder(&b)
		enc.SetEscapeHTML(false)
		_ = enc.Encode(r)
		sum := sha256.Sum256(append([]byte("tokenscope/canonical-record/v1\x00"), []byte(strings.TrimSuffix(b.String(), "\n"))...))
		return hub.UsageEnvelope{Record: r, RecordRevision: rev, PayloadHash: hex.EncodeToString(sum[:])}
	}
	first := makeEnv(1, 10)
	if _, err := repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{first}, 120); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{first}, 130); err != nil {
		t.Fatal(err)
	}
	second := makeEnv(2, 20)
	if _, err := repo.ApplyUsageBatch(ctx, device.DeviceID, []hub.UsageEnvelope{second}, 140); err != nil {
		t.Fatal(err)
	}
	page, high, err := repo.PullUsageChanges(ctx, 0, 1)
	if err != nil {
		t.Fatal(err)
	}
	if high != 2 || len(page) != 1 || page[0].Envelope.RecordRevision != 1 {
		t.Fatalf("page=%+v high=%d", page, high)
	}
	page2, high, err := repo.PullUsageChanges(ctx, page[0].ChangeSeq, 10)
	if err != nil {
		t.Fatal(err)
	}
	if high != 2 || len(page2) != 1 || page2[0].Envelope.RecordRevision != 2 {
		t.Fatalf("page2=%+v high=%d", page2, high)
	}
}
