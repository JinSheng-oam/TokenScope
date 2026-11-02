package httpapi

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"example.com/tokenscope/hub/internal/hub"
)

func TestUsageBatchHTTPIdempotentAndDeviceScoped(t *testing.T) {
	repo := hub.NewMemoryRepository()
	handler, err := NewHandler(repo, Config{AdminToken: testAdminToken, Now: func() time.Time { return time.UnixMilli(1_800_000_000_000).UTC() }})
	if err != nil {
		t.Fatal(err)
	}
	svc := hub.NewService(repo)
	code, err := svc.CreatePairingCode(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	pair, err := svc.Pair(context.Background(), hub.PairRequest{PairingCode: code.PairingCode, DeviceID: "office-pc-01", DisplayName: "Office PC", OSName: "windows", AppVersion: "0.1.0", TimeZone: "UTC"})
	if err != nil {
		t.Fatal(err)
	}
	rec := hub.UsageRecord{RecordID: strings.Repeat("a", 64), OwnerDeviceID: pair.DeviceID, ToolID: "codex", Scope: "device_local", Precision: "exact", Usage: hub.UsageBreakdown{OutputTokens: 10}}
	var b strings.Builder
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(rec); err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(append([]byte("tokenscope/canonical-record/v1\x00"), []byte(strings.TrimSuffix(b.String(), "\n"))...))
	env := hub.UsageEnvelope{Record: rec, RecordRevision: 1, PayloadHash: hex.EncodeToString(sum[:])}
	req := hub.UsageBatchRequest{ProtocolVersion: hub.ProtocolVersion, BatchID: "batch-000000000001", Records: []hub.UsageEnvelope{env}}
	raw, _ := json.Marshal(req)
	w := request(handler, "POST", "/api/v1/usage/batch", string(raw), pair.DeviceToken)
	if w.Code != 200 {
		t.Fatalf("push: %d %s", w.Code, w.Body.String())
	}
	var resp hub.UsageBatchResponse
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatal(err)
	}
	if resp.Results[0].Status != "accepted" {
		t.Fatalf("status: %#v", resp.Results[0])
	}
	w = request(handler, "POST", "/api/v1/usage/batch", string(raw), pair.DeviceToken)
	if w.Code != 200 {
		t.Fatalf("repeat: %d %s", w.Code, w.Body.String())
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatal(err)
	}
	if resp.Results[0].Status != "unchanged" {
		t.Fatalf("repeat status: %#v", resp.Results[0])
	}
	pull := request(handler, "GET", "/api/v1/usage/changes?cursor=0&limit=10", "", pair.DeviceToken)
	if pull.Code != 200 {
		t.Fatalf("pull status %d: %s", pull.Code, pull.Body.String())
	}
	var page hub.UsageChangesResponse
	if err := json.Unmarshal(pull.Body.Bytes(), &page); err != nil {
		t.Fatal(err)
	}
	if len(page.Changes) != 1 || page.NextCursor != 1 || page.HighWatermark != 1 || page.HasMore {
		t.Fatalf("unexpected change page: %+v", page)
	}
	if page.Changes[0].Envelope.RecordRevision != 1 || page.Changes[0].OwnerDeviceID != pair.DeviceID {
		t.Fatalf("unexpected change: %+v", page.Changes[0])
	}
	rec.OwnerDeviceID = "other-pc-001"
	env.Record = rec
	req.Records = []hub.UsageEnvelope{env}
	raw, _ = json.Marshal(req)
	w = request(handler, "POST", "/api/v1/usage/batch", string(raw), pair.DeviceToken)
	if w.Code != 400 {
		t.Fatalf("cross-device upload status %d: %s", w.Code, w.Body.String())
	}
}

func TestPullUsageChangesRequiresDeviceAndUsesCursor(t *testing.T) {
	repo := hub.NewMemoryRepository()
	handler, err := NewHandler(repo, Config{AdminToken: testAdminToken})
	if err != nil {
		t.Fatal(err)
	}
	svc := hub.NewService(repo)
	code, _ := svc.CreatePairingCode(context.Background())
	pair, err := svc.Pair(context.Background(), hub.PairRequest{PairingCode: code.PairingCode, DeviceID: "office-pc-01", DisplayName: "Office PC", OSName: "windows", AppVersion: "0.1.0", TimeZone: "UTC"})
	if err != nil {
		t.Fatal(err)
	}
	w := request(handler, "GET", "/api/v1/usage/changes?cursor=0&limit=10", "", pair.DeviceToken)
	if w.Code != 200 {
		t.Fatalf("pull: %d %s", w.Code, w.Body.String())
	}
	w = request(handler, "GET", "/api/v1/usage/changes?cursor=-1", "", pair.DeviceToken)
	if w.Code != 400 {
		t.Fatalf("negative cursor: %d", w.Code)
	}
	w = request(handler, "GET", "/api/v1/usage/changes?cursor=0", "", "")
	if w.Code != 401 {
		t.Fatalf("unauthenticated pull: %d", w.Code)
	}
}
