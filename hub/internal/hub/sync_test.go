package hub

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"
)

func testEnvelope(device string, revision uint64, output uint64) UsageEnvelope {
	r := UsageRecord{RecordID: strings.Repeat("a", 64), OwnerDeviceID: device, ToolID: "codex", Scope: "device_local", Precision: "exact", Usage: UsageBreakdown{OutputTokens: output}}
	raw, _ := canonicalUsageJSON(r)
	sum := sha256.Sum256(append([]byte("tokenscope/canonical-record/v1\x00"), raw...))
	return UsageEnvelope{Record: r, RecordRevision: revision, PayloadHash: hex.EncodeToString(sum[:])}
}
func TestUsageBatchIdempotencyAndRevisionCAS(t *testing.T) {
	s, repo := testService()
	code, _ := s.CreatePairingCode(context.Background())
	pair, err := s.Pair(context.Background(), validPairRequest(code.PairingCode, "office-pc-01"))
	if err != nil {
		t.Fatal(err)
	}
	first := testEnvelope(pair.DeviceID, 1, 10)
	got, err := s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{first})
	if err != nil {
		t.Fatal(err)
	}
	if got[0].Status != "accepted" {
		t.Fatal(got)
	}
	got, err = s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{first})
	if err != nil {
		t.Fatal(err)
	}
	if got[0].Status != "unchanged" {
		t.Fatal(got)
	}
	newer := testEnvelope(pair.DeviceID, 2, 20)
	got, err = s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{newer})
	if err != nil {
		t.Fatal(err)
	}
	if got[0].Status != "accepted" {
		t.Fatal(got)
	}
	got, err = s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{first})
	if err != nil {
		t.Fatal(err)
	}
	if got[0].Status != "superseded" || got[0].HubRevision != 2 {
		t.Fatal(got)
	}
	if len(repo.usage) != 1 {
		t.Fatalf("stored rows=%d", len(repo.usage))
	}
}
func TestUsageBatchRejectsCrossDeviceAndBadHash(t *testing.T) {
	s, _ := testService()
	code, _ := s.CreatePairingCode(context.Background())
	pair, _ := s.Pair(context.Background(), validPairRequest(code.PairingCode, "office-pc-01"))
	bad := testEnvelope("other-pc-001", 1, 10)
	if _, err := s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{bad}); err == nil {
		t.Fatal("cross-device upload accepted")
	}
	bad = testEnvelope(pair.DeviceID, 1, 10)
	bad.PayloadHash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	if _, err := s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{bad}); err == nil {
		t.Fatal("bad hash accepted")
	}
}
func TestCanonicalWireJSONOrder(t *testing.T) {
	r := testEnvelope("office-pc-01", 1, 1).Record
	b, err := json.Marshal(r)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(b, []byte(`"reasoning_tokens":null`)) {
		t.Fatal(string(b))
	}
}

func TestRustCanonicalRecordFixtureHashParity(t *testing.T) {
	data, err := os.ReadFile("../../../fixtures/domain/canonical-record-v1.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		Record      UsageRecord `json:"record"`
		PayloadHash string      `json:"payload_hash"`
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	raw, err := canonicalUsageJSON(fixture.Record)
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(append([]byte("tokenscope/canonical-record/v1\x00"), raw...))
	if got := hex.EncodeToString(sum[:]); got != fixture.PayloadHash {
		t.Fatalf("cross-language canonical hash mismatch: got %s want %s; payload=%s", got, fixture.PayloadHash, raw)
	}
}

func TestMemoryBatchConflictIsAtomic(t *testing.T) {
	s, repo := testService()
	code, _ := s.CreatePairingCode(context.Background())
	pair, err := s.Pair(context.Background(), validPairRequest(code.PairingCode, "office-pc-01"))
	if err != nil {
		t.Fatal(err)
	}
	existing := testEnvelope(pair.DeviceID, 1, 10)
	if _, err := s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{existing}); err != nil {
		t.Fatal(err)
	}
	fresh := testEnvelope(pair.DeviceID, 1, 11)
	fresh.Record.RecordID = strings.Repeat("b", 64)
	raw, _ := canonicalUsageJSON(fresh.Record)
	sum := sha256.Sum256(append([]byte("tokenscope/canonical-record/v1\x00"), raw...))
	fresh.PayloadHash = hex.EncodeToString(sum[:])
	conflict := testEnvelope(pair.DeviceID, 1, 12)
	if _, err := s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{fresh, conflict}); !errors.Is(err, ErrRevisionConflict) {
		t.Fatalf("expected conflict, got %v", err)
	}
	if _, ok := repo.usage[pair.DeviceID+":"+fresh.Record.RecordID]; ok {
		t.Fatal("batch partially committed before conflict")
	}
}

func TestUsageChangeFeedIncludesAcceptedRevisionsOnly(t *testing.T) {
	s, _ := testService()
	code, _ := s.CreatePairingCode(context.Background())
	pair, err := s.Pair(context.Background(), validPairRequest(code.PairingCode, "office-pc-01"))
	if err != nil {
		t.Fatal(err)
	}
	first := testEnvelope(pair.DeviceID, 1, 10)
	if _, err = s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{first}); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{first}); err != nil {
		t.Fatal(err)
	} // unchanged must not emit a feed row
	second := testEnvelope(pair.DeviceID, 2, 20)
	if _, err = s.ApplyUsageBatch(context.Background(), pair.DeviceID, []UsageEnvelope{second}); err != nil {
		t.Fatal(err)
	}
	page, err := s.PullUsageChanges(context.Background(), 0, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(page.Changes) != 1 || page.Changes[0].Envelope.RecordRevision != 1 || !page.HasMore || page.NextCursor != 1 {
		t.Fatalf("page1=%+v", page)
	}
	page2, err := s.PullUsageChanges(context.Background(), page.NextCursor, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(page2.Changes) != 1 || page2.Changes[0].Envelope.RecordRevision != 2 || page2.HasMore || page2.NextCursor != 2 {
		t.Fatalf("page2=%+v", page2)
	}
}
