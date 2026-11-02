package httpapi

import (
	"crypto/sha256"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"strconv"
	"strings"
	"time"

	"example.com/tokenscope/hub/internal/hub"
)

const maxRequestBytes = 2 * 1024 * 1024

type Config struct {
	AdminToken        string
	TrustedProxyCIDRs []string
	Now               func() time.Time
}

type Server struct {
	service *hub.Service
	config  Config
	proxies []*net.IPNet
	limiter *pairLimiter
}

type deviceContextKey struct{}

func NewHandler(repo hub.Repository, cfg Config) (http.Handler, error) {
	if len(cfg.AdminToken) < 32 {
		return nil, errors.New("TOKENSCOPE_ADMIN_TOKEN must contain at least 32 characters")
	}
	if cfg.Now == nil {
		cfg.Now = time.Now
	}
	proxies := make([]*net.IPNet, 0, len(cfg.TrustedProxyCIDRs))
	for _, cidr := range cfg.TrustedProxyCIDRs {
		_, network, err := net.ParseCIDR(strings.TrimSpace(cidr))
		if err != nil {
			return nil, errors.New("invalid trusted proxy CIDR")
		}
		proxies = append(proxies, network)
	}
	s := &Server{service: hub.NewService(repo), config: cfg, proxies: proxies, limiter: newPairLimiter()}
	s.service.SetClock(cfg.Now)
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", s.health)
	mux.Handle("POST /api/v1/pair", s.requireTransport(http.HandlerFunc(s.pair)))
	mux.Handle("POST /api/v1/usage/batch", s.requireTransport(s.requireDevice(http.HandlerFunc(s.pushUsageBatch))))
	mux.Handle("GET /api/v1/usage/changes", s.requireTransport(s.requireDevice(http.HandlerFunc(s.pullUsageChanges))))
	mux.Handle("POST /api/v1/admin/pairing-codes", s.requireTransport(s.requireAdmin(http.HandlerFunc(s.createPairingCode))))
	mux.Handle("GET /api/v1/devices", s.requireTransport(s.requireDevice(http.HandlerFunc(s.listDevices))))
	mux.Handle("PATCH /api/v1/devices/self", s.requireTransport(s.requireDevice(http.HandlerFunc(s.renameSelf))))
	mux.Handle("POST /api/v1/devices/self/rotate-token", s.requireTransport(s.requireDevice(http.HandlerFunc(s.rotateSelfToken))))
	mux.Handle("POST /api/v1/admin/devices/{id}/revoke", s.requireTransport(s.requireAdmin(http.HandlerFunc(s.revokeDevice))))
	return s.securityHeaders(mux), nil
}

func (s *Server) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"status": "ok", "protocol_version": hub.ProtocolVersion})
}

func (s *Server) createPairingCode(w http.ResponseWriter, r *http.Request) {
	var req struct{}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_json")
		return
	}
	result, err := s.service.CreatePairingCode(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "pairing_code_creation_failed")
		return
	}
	writeJSON(w, http.StatusCreated, result)
}

func (s *Server) pair(w http.ResponseWriter, r *http.Request) {
	if !s.limiter.Allow(remoteIP(r)) {
		writeError(w, http.StatusTooManyRequests, "pair_rate_limited")
		return
	}
	var req hub.PairRequest
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_json")
		return
	}
	result, err := s.service.Pair(r.Context(), req)
	if err != nil {
		s.writeServiceError(w, err)
		return
	}
	writeJSON(w, http.StatusCreated, result)
}

func (s *Server) listDevices(w http.ResponseWriter, r *http.Request) {
	devices, err := s.service.ListDevices(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "device_list_failed")
		return
	}
	writeJSON(w, http.StatusOK, hub.DeviceListResponse{ProtocolVersion: hub.ProtocolVersion, Devices: devices})
}

func (s *Server) renameSelf(w http.ResponseWriter, r *http.Request) {
	device := deviceFromContext(r)
	var req struct {
		DisplayName string `json:"display_name"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_json")
		return
	}
	if err := s.service.RenameDevice(r.Context(), device.DeviceID, req.DisplayName); err != nil {
		s.writeServiceError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"protocol_version": hub.ProtocolVersion, "device_id": device.DeviceID, "display_name": strings.TrimSpace(req.DisplayName)})
}

func (s *Server) rotateSelfToken(w http.ResponseWriter, r *http.Request) {
	device := deviceFromContext(r)
	result, err := s.service.RotateCredential(r.Context(), device.DeviceID, bearer(r))
	if err != nil {
		s.writeServiceError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, result)
}

func (s *Server) revokeDevice(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	if err := s.service.RevokeDevice(r.Context(), id); err != nil {
		s.writeServiceError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"protocol_version": hub.ProtocolVersion, "device_id": id, "revoked": true})
}

func (s *Server) requireAdmin(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		provided := bearer(r)
		if provided == "" || !constantTimeSecretEqual(provided, s.config.AdminToken) {
			writeError(w, http.StatusUnauthorized, "admin_auth_required")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func (s *Server) requireDevice(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		token := bearer(r)
		if token == "" {
			writeError(w, http.StatusUnauthorized, "device_auth_required")
			return
		}
		device, err := s.service.Authenticate(r.Context(), token)
		if err != nil {
			s.writeServiceError(w, err)
			return
		}
		ctx := contextWithDevice(r, device)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func (s *Server) requireTransport(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.TLS != nil {
			next.ServeHTTP(w, r)
			return
		}
		forwarded := r.Header.Get("X-Forwarded-Proto") != "" || r.Header.Get("X-Forwarded-For") != ""
		if s.isTrustedProxy(r.RemoteAddr) && strings.EqualFold(r.Header.Get("X-Forwarded-Proto"), "https") {
			next.ServeHTTP(w, r)
			return
		}
		ip := parseRemoteIP(r.RemoteAddr)
		if ip != nil && ip.IsLoopback() && !forwarded {
			next.ServeHTTP(w, r)
			return
		}
		writeError(w, http.StatusUpgradeRequired, "https_required")
	})
}
func (s *Server) isTrustedProxy(remote string) bool {
	ip := parseRemoteIP(remote)
	if ip == nil {
		return false
	}
	for _, n := range s.proxies {
		if n.Contains(ip) {
			return true
		}
	}
	return false
}
func parseRemoteIP(remote string) net.IP {
	host, _, err := net.SplitHostPort(remote)
	if err != nil {
		host = remote
	}
	return net.ParseIP(strings.Trim(host, "[]"))
}
func remoteIP(r *http.Request) string {
	if ip := parseRemoteIP(r.RemoteAddr); ip != nil {
		return ip.String()
	}
	return "unknown"
}

func (s *Server) securityHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Cache-Control", "no-store")
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("Content-Security-Policy", "default-src 'none'; frame-ancestors 'none'")
		next.ServeHTTP(w, r)
	})
}

func (s *Server) writeServiceError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, hub.ErrInvalidPairingCode):
		writeError(w, http.StatusUnauthorized, "pairing_code_invalid_or_expired")
	case errors.Is(err, hub.ErrDeviceExists):
		writeError(w, http.StatusConflict, "device_already_registered")
	case errors.Is(err, hub.ErrInvalidCredential):
		writeError(w, http.StatusUnauthorized, "device_credential_invalid")
	case errors.Is(err, hub.ErrDeviceRevoked):
		writeError(w, http.StatusForbidden, "device_revoked")
	case errors.Is(err, hub.ErrDeviceNotFound):
		writeError(w, http.StatusNotFound, "device_not_found")
	case errors.Is(err, hub.ErrInvalidInput):
		writeError(w, http.StatusBadRequest, "invalid_input")
	case errors.Is(err, hub.ErrRevisionConflict):
		writeError(w, http.StatusConflict, "revision_conflict")
	case errors.Is(err, hub.ErrPayloadHashMismatch):
		writeError(w, http.StatusBadRequest, "payload_hash_mismatch")
	case errors.Is(err, hub.ErrCredentialConflict):
		writeError(w, http.StatusConflict, "credential_conflict")
	default:
		writeError(w, http.StatusInternalServerError, "internal_error")
	}
}

func bearer(r *http.Request) string {
	h := r.Header.Get("Authorization")
	if len(h) < 8 || !strings.EqualFold(h[:7], "Bearer ") {
		return ""
	}
	return strings.TrimSpace(h[7:])
}
func constantTimeSecretEqual(a, b string) bool {
	ha := sha256.Sum256([]byte(a))
	hb := sha256.Sum256([]byte(b))
	return subtle.ConstantTimeCompare(ha[:], hb[:]) == 1
}
func decodeJSON(w http.ResponseWriter, r *http.Request, dst any) error {
	r.Body = http.MaxBytesReader(w, r.Body, maxRequestBytes)
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(dst); err != nil {
		return err
	}
	var extra any
	if err := dec.Decode(&extra); err != io.EOF {
		return errors.New("trailing JSON data")
	}
	return nil
}
func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}
func writeError(w http.ResponseWriter, status int, code string) {
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code}})
}

func (s *Server) pushUsageBatch(w http.ResponseWriter, r *http.Request) {
	device := deviceFromContext(r)
	var req hub.UsageBatchRequest
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid_json")
		return
	}
	if req.ProtocolVersion != hub.ProtocolVersion || len(req.BatchID) < 16 || len(req.BatchID) > 128 || !safeBatchID(req.BatchID) || len(req.Records) == 0 || len(req.Records) > 200 {
		writeError(w, http.StatusBadRequest, "invalid_batch")
		return
	}
	results, err := s.service.ApplyUsageBatch(r.Context(), device.DeviceID, req.Records)
	if err != nil {
		s.writeServiceError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, hub.UsageBatchResponse{ProtocolVersion: hub.ProtocolVersion, BatchID: req.BatchID, Results: results})
}

func safeBatchID(value string) bool {
	for _, c := range value {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}

func (s *Server) pullUsageChanges(w http.ResponseWriter, r *http.Request) {
	cursor, err := strconv.ParseInt(r.URL.Query().Get("cursor"), 10, 64)
	if err != nil || cursor < 0 {
		writeError(w, http.StatusBadRequest, "invalid_cursor")
		return
	}
	limit := 200
	if raw := r.URL.Query().Get("limit"); raw != "" {
		n, e := strconv.Atoi(raw)
		if e != nil || n < 1 || n > 500 {
			writeError(w, http.StatusBadRequest, "invalid_limit")
			return
		}
		limit = n
	}
	result, err := s.service.PullUsageChanges(r.Context(), cursor, limit)
	if err != nil {
		s.writeServiceError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, result)
}
