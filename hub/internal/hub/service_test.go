package hub

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"
)

func testService() (*Service, *MemoryRepository) {
	r := NewMemoryRepository()
	s := NewService(r)
	s.SetClock(func() time.Time { return time.UnixMilli(1_800_000_000_000).UTC() })
	return s, r
}
func validPairRequest(code, id string) PairRequest {
	return PairRequest{PairingCode: code, DeviceID: id, DisplayName: "Office PC", OSName: "windows", AppVersion: "0.1.0", TimeZone: "Asia/Shanghai"}
}

func TestPairingCodeIsSingleUseAndTokenAuthenticates(t *testing.T) {
	s, _ := testService()
	code, err := s.CreatePairingCode(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(code.PairingCode) != 52 {
		t.Fatalf("expected 52-char code, got %d", len(code.PairingCode))
	}
	paired, err := s.Pair(context.Background(), validPairRequest(strings.ToLower(code.PairingCode), "office-pc-01"))
	if err != nil {
		t.Fatal(err)
	}
	if len(paired.DeviceToken) != 43 || paired.DeviceID != "office-pc-01" {
		t.Fatalf("bad pair response: %#v", paired)
	}
	device, err := s.Authenticate(context.Background(), paired.DeviceToken)
	if err != nil {
		t.Fatal(err)
	}
	if device.DeviceID != "office-pc-01" || device.LastSeenMS == nil {
		t.Fatalf("bad authenticated device: %#v", device)
	}
	if _, err = s.Pair(context.Background(), validPairRequest(code.PairingCode, "office-pc-02")); !errors.Is(err, ErrInvalidPairingCode) {
		t.Fatalf("pairing code was reusable: %v", err)
	}
}

func TestExpiredPairingCodeRejected(t *testing.T) {
	s, _ := testService()
	code, err := s.CreatePairingCode(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	s.SetClock(func() time.Time { return time.UnixMilli(code.ExpiresAtMS).UTC() })
	if _, err = s.Pair(context.Background(), validPairRequest(code.PairingCode, "office-pc-01")); !errors.Is(err, ErrInvalidPairingCode) {
		t.Fatalf("expected expired code, got %v", err)
	}
}

func TestDeviceTokenRotationRevocationAndRename(t *testing.T) {
	s, _ := testService()
	code, _ := s.CreatePairingCode(context.Background())
	pair, err := s.Pair(context.Background(), validPairRequest(code.PairingCode, "office-pc-01"))
	if err != nil {
		t.Fatal(err)
	}
	rotated, err := s.RotateCredential(context.Background(), pair.DeviceID, pair.DeviceToken)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Authenticate(context.Background(), pair.DeviceToken); !errors.Is(err, ErrInvalidCredential) {
		t.Fatalf("old token still valid: %v", err)
	}
	if _, err = s.Authenticate(context.Background(), rotated.DeviceToken); err != nil {
		t.Fatalf("new token invalid: %v", err)
	}
	if err = s.RenameDevice(context.Background(), pair.DeviceID, "My Mac"); err != nil {
		t.Fatal(err)
	}
	if err = s.RevokeDevice(context.Background(), pair.DeviceID); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Authenticate(context.Background(), rotated.DeviceToken); !errors.Is(err, ErrDeviceRevoked) {
		t.Fatalf("revoked token accepted: %v", err)
	}
	if err = s.RenameDevice(context.Background(), pair.DeviceID, "Renamed"); !errors.Is(err, ErrDeviceRevoked) {
		t.Fatalf("renamed revoked device: %v", err)
	}
}

func TestPairValidationRejectsUnsafeMetadata(t *testing.T) {
	s, _ := testService()
	code, _ := s.CreatePairingCode(context.Background())
	cases := []PairRequest{
		func() PairRequest {
			r := validPairRequest(code.PairingCode, "office-pc-01")
			r.OSName = "unknown"
			return r
		}(),
		func() PairRequest {
			r := validPairRequest(code.PairingCode, "office-pc-01")
			r.TimeZone = "../../etc/passwd"
			return r
		}(),
		func() PairRequest {
			r := validPairRequest(code.PairingCode, "office-pc-01")
			r.DisplayName = "Office\nPC"
			return r
		}(),
		func() PairRequest { r := validPairRequest(code.PairingCode, "short"); return r }(),
	}
	for i, r := range cases {
		if _, err := s.Pair(context.Background(), r); !errors.Is(err, ErrInvalidInput) {
			t.Errorf("case %d expected invalid input, got %v", i, err)
		}
	}
}

func TestConcurrentPairConsumesCodeOnlyOnce(t *testing.T) {
	s, _ := testService()
	code, _ := s.CreatePairingCode(context.Background())
	var wg sync.WaitGroup
	successes := 0
	mu := sync.Mutex{}
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			id := []string{"office-pc-01", "office-pc-02", "office-pc-03", "office-pc-04"}[i%4]
			_, err := s.Pair(context.Background(), validPairRequest(code.PairingCode, id))
			if err == nil {
				mu.Lock()
				successes++
				mu.Unlock()
			}
		}(i)
	}
	wg.Wait()
	if successes != 1 {
		t.Fatalf("pairing code consumed %d times, want exactly once", successes)
	}
}

func TestSecretStringersAreRedacted(t *testing.T) {
	request := PairRequest{PairingCode: "PAIRING-CODE-SECRET", DeviceID: "office-pc-01", DisplayName: "Office", OSName: "windows", AppVersion: "0.1.0", TimeZone: "UTC"}
	response := PairResponse{ProtocolVersion: 1, DeviceID: "office-pc-01", DeviceToken: "DEVICE-TOKEN-SECRET", CreatedAtMS: 1}
	code := PairingCodeResponse{PairingCode: "PAIRING-CODE-SECRET", ExpiresAtMS: 2}
	for _, value := range []fmt.Stringer{request, response, code} {
		if strings.Contains(value.String(), "SECRET") {
			t.Fatalf("secret leaked by String(): %s", value.String())
		}
	}
}
