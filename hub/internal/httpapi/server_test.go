package httpapi

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"example.com/tokenscope/hub/internal/hub"
)

const testAdminToken = "test-admin-token-with-at-least-32-characters"

func testHandler(t *testing.T) http.Handler {
	t.Helper()
	repo := hub.NewMemoryRepository()
	h, err := NewHandler(repo, Config{AdminToken: testAdminToken, Now: func() time.Time { return time.UnixMilli(1_800_000_000_000).UTC() }})
	if err != nil {
		t.Fatal(err)
	}
	return h
}
func request(h http.Handler, method, path, body, token string) *httptest.ResponseRecorder {
	r := httptest.NewRequest(method, path, strings.NewReader(body))
	r.RemoteAddr = "127.0.0.1:34567"
	if token != "" {
		r.Header.Set("Authorization", "Bearer "+token)
	}
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	return w
}
func TestPairListRenameRotateAndRevokeHTTP(t *testing.T) {
	h := testHandler(t)
	w := request(h, "POST", "/api/v1/admin/pairing-codes", "{}", testAdminToken)
	if w.Code != http.StatusCreated {
		t.Fatalf("create code: %d %s", w.Code, w.Body.String())
	}
	var code hub.PairingCodeResponse
	if err := json.Unmarshal(w.Body.Bytes(), &code); err != nil {
		t.Fatal(err)
	}
	payload, _ := json.Marshal(hub.PairRequest{PairingCode: code.PairingCode, DeviceID: "office-pc-01", DisplayName: "Office PC", OSName: "windows", AppVersion: "0.1.0", TimeZone: "UTC"})
	w = request(h, "POST", "/api/v1/pair", string(payload), "")
	if w.Code != http.StatusCreated {
		t.Fatalf("pair: %d %s", w.Code, w.Body.String())
	}
	var pair hub.PairResponse
	if err := json.Unmarshal(w.Body.Bytes(), &pair); err != nil {
		t.Fatal(err)
	}
	w = request(h, "GET", "/api/v1/devices", "", pair.DeviceToken)
	if w.Code != http.StatusOK || !bytes.Contains(w.Body.Bytes(), []byte("office-pc-01")) {
		t.Fatalf("list devices: %d %s", w.Code, w.Body.String())
	}
	w = request(h, "PATCH", "/api/v1/devices/self", `{"display_name":"My Work PC"}`, pair.DeviceToken)
	if w.Code != http.StatusOK {
		t.Fatalf("rename: %d %s", w.Code, w.Body.String())
	}
	w = request(h, "POST", "/api/v1/devices/self/rotate-token", "", pair.DeviceToken)
	if w.Code != http.StatusOK {
		t.Fatalf("rotate: %d %s", w.Code, w.Body.String())
	}
	var rotated hub.RotateTokenResponse
	if err := json.Unmarshal(w.Body.Bytes(), &rotated); err != nil {
		t.Fatal(err)
	}
	w = request(h, "GET", "/api/v1/devices", "", pair.DeviceToken)
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("old token status %d", w.Code)
	}
	w = request(h, "POST", "/api/v1/admin/devices/office-pc-01/revoke", "", testAdminToken)
	if w.Code != http.StatusOK {
		t.Fatalf("revoke: %d %s", w.Code, w.Body.String())
	}
	w = request(h, "GET", "/api/v1/devices", "", rotated.DeviceToken)
	if w.Code != http.StatusForbidden {
		t.Fatalf("revoked token status %d body %s", w.Code, w.Body.String())
	}
}
func TestAdminAuthAndStrictJSON(t *testing.T) {
	h := testHandler(t)
	w := request(h, "POST", "/api/v1/admin/pairing-codes", "{}", "wrong-token")
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("admin auth status %d", w.Code)
	}
	w = request(h, "POST", "/api/v1/admin/pairing-codes", `{"unexpected":true}`, testAdminToken)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("unknown field status %d", w.Code)
	}
}
func TestHTTPSRequiredForRemoteAndTrustedProxy(t *testing.T) {
	repo := hub.NewMemoryRepository()
	h, err := NewHandler(repo, Config{AdminToken: testAdminToken, TrustedProxyCIDRs: []string{"10.10.0.0/16"}})
	if err != nil {
		t.Fatal(err)
	}
	r := httptest.NewRequest("POST", "/api/v1/admin/pairing-codes", strings.NewReader("{}"))
	r.RemoteAddr = "203.0.113.10:1234"
	r.Header.Set("Authorization", "Bearer "+testAdminToken)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, r)
	if w.Code != http.StatusUpgradeRequired {
		t.Fatalf("remote plaintext status %d", w.Code)
	}
	// Forwarding headers from an untrusted remote must not bypass TLS enforcement.
	r = httptest.NewRequest("POST", "/api/v1/admin/pairing-codes", strings.NewReader("{}"))
	r.RemoteAddr = "203.0.113.10:1234"
	r.Header.Set("X-Forwarded-Proto", "https")
	r.Header.Set("Authorization", "Bearer "+testAdminToken)
	w = httptest.NewRecorder()
	h.ServeHTTP(w, r)
	if w.Code != http.StatusUpgradeRequired {
		t.Fatalf("untrusted forwarded header bypassed TLS: %d", w.Code)
	}
	r = httptest.NewRequest("POST", "/api/v1/admin/pairing-codes", strings.NewReader("{}"))
	r.RemoteAddr = "10.10.0.5:443"
	r.Header.Set("X-Forwarded-Proto", "https")
	r.Header.Set("Authorization", "Bearer "+testAdminToken)
	w = httptest.NewRecorder()
	h.ServeHTTP(w, r)
	if w.Code != http.StatusCreated {
		t.Fatalf("trusted proxy https status %d: %s", w.Code, w.Body.String())
	}
}
func TestPairRateLimit(t *testing.T) {
	h := testHandler(t)
	for i := 0; i < 10; i++ {
		w := request(h, "POST", "/api/v1/pair", `{}`, "")
		if w.Code != http.StatusUnauthorized {
			t.Fatalf("attempt %d status %d", i, w.Code)
		}
	}
	w := request(h, "POST", "/api/v1/pair", `{}`, "")
	if w.Code != http.StatusTooManyRequests {
		t.Fatalf("rate limit status %d", w.Code)
	}
}
