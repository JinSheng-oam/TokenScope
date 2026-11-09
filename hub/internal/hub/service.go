package hub

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base32"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"
)

var deviceIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_-]{7,127}$`)
var safeVersionPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9.+_-]{0,63}$`)

type Service struct {
	repo Repository
	now  func() time.Time
}

func NewService(repo Repository) *Service {
	return &Service{repo: repo, now: time.Now}
}

// SetClock is intended for deterministic tests; production code should use
// NewService and the system clock.
func (s *Service) SetClock(now func() time.Time) { s.now = now }

func hashSecret(raw string) string {
	sum := sha256.Sum256([]byte(raw))
	return hex.EncodeToString(sum[:])
}

func (s *Service) CreatePairingCode(ctx context.Context) (PairingCodeResponse, error) {
	var raw [32]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return PairingCodeResponse{}, fmt.Errorf("generate pairing code: %w", err)
	}
	code := base32.StdEncoding.WithPadding(base32.NoPadding).EncodeToString(raw[:])
	now := s.now().UTC()
	expires := now.Add(PairingCodeTTL)
	if err := s.repo.CreatePairingCode(ctx, hashSecret(code), now.UnixMilli(), expires.UnixMilli()); err != nil {
		return PairingCodeResponse{}, err
	}
	return PairingCodeResponse{PairingCode: code, ExpiresAtMS: expires.UnixMilli()}, nil
}

func (s *Service) Pair(ctx context.Context, req PairRequest) (PairResponse, error) {
	if err := validatePairRequest(req); err != nil {
		return PairResponse{}, err
	}
	var tokenBytes [32]byte
	if _, err := rand.Read(tokenBytes[:]); err != nil {
		return PairResponse{}, fmt.Errorf("generate device credential: %w", err)
	}
	token := base64.RawURLEncoding.EncodeToString(tokenBytes[:])
	now := s.now().UTC().UnixMilli()
	device := Device{
		DeviceID: req.DeviceID, DisplayName: req.DisplayName, OSName: req.OSName,
		AppVersion: req.AppVersion, TimeZone: req.TimeZone, CreatedAtMS: now,
	}
	if err := s.repo.PairDevice(ctx, hashSecret(normalizePairingCode(req.PairingCode)), now, device, hashSecret(token)); err != nil {
		return PairResponse{}, err
	}
	return PairResponse{ProtocolVersion: ProtocolVersion, DeviceID: req.DeviceID, DeviceToken: token, CreatedAtMS: now}, nil
}

func (s *Service) Authenticate(ctx context.Context, token string) (Device, error) {
	if len(token) < 40 || len(token) > 128 {
		return Device{}, ErrInvalidCredential
	}
	return s.repo.Authenticate(ctx, hashSecret(token), s.now().UTC().UnixMilli())
}

func (s *Service) ListDevices(ctx context.Context) ([]Device, error) {
	return s.repo.ListDevices(ctx)
}

func (s *Service) UpsertProjectNames(ctx context.Context, deviceID string, inputs []ProjectNameInput) (int, error) {
	if !deviceIDPattern.MatchString(deviceID) || len(inputs) == 0 || len(inputs) > 200 {
		return 0, ErrInvalidInput
	}
	names := make([]ProjectName, 0, len(inputs))
	seen := make(map[string]struct{}, len(inputs))
	nowMS := s.now().UTC().UnixMilli()
	for _, input := range inputs {
		name := strings.TrimSpace(input.Name)
		if !validOpaque(input.ProjectID, "prj_") || !validProjectDisplayName(name) {
			return 0, ErrInvalidInput
		}
		if _, ok := seen[input.ProjectID]; ok {
			return 0, ErrInvalidInput
		}
		seen[input.ProjectID] = struct{}{}
		names = append(names, ProjectName{
			OwnerDeviceID: deviceID,
			ProjectID:     input.ProjectID,
			Name:          name,
			UpdatedAtMS:   nowMS,
		})
	}
	if err := s.repo.UpsertProjectNames(ctx, deviceID, names, nowMS); err != nil {
		return 0, err
	}
	return len(names), nil
}

func (s *Service) ListProjectNames(ctx context.Context, afterDeviceID, afterProjectID string, limit int) (ProjectNamesPage, error) {
	if limit < 1 || limit > 500 {
		return ProjectNamesPage{}, ErrInvalidInput
	}
	if (afterDeviceID == "") != (afterProjectID == "") ||
		(afterDeviceID != "" && (!deviceIDPattern.MatchString(afterDeviceID) || !validOpaque(afterProjectID, "prj_"))) {
		return ProjectNamesPage{}, ErrInvalidInput
	}
	names, hasMore, err := s.repo.ListProjectNames(ctx, afterDeviceID, afterProjectID, limit)
	if err != nil {
		return ProjectNamesPage{}, err
	}
	page := ProjectNamesPage{
		ProtocolVersion: ProtocolVersion,
		ProjectNames:    names,
		NextDeviceID:    afterDeviceID,
		NextProjectID:   afterProjectID,
		HasMore:         hasMore,
	}
	if len(names) > 0 {
		last := names[len(names)-1]
		page.NextDeviceID = last.OwnerDeviceID
		page.NextProjectID = last.ProjectID
	}
	return page, nil
}

func validProjectDisplayName(name string) bool {
	return name != "." && name != ".." && !strings.ContainsAny(name, "/\\") &&
		utf8.RuneCountInString(name) <= 120 && validUsageText(name, 512)
}

func (s *Service) RenameDevice(ctx context.Context, id, name string) error {
	if !deviceIDPattern.MatchString(id) || !validDisplayName(name) {
		return ErrInvalidInput
	}
	return s.repo.RenameDevice(ctx, id, strings.TrimSpace(name))
}

func (s *Service) RotateCredential(ctx context.Context, deviceID, oldToken string) (RotateTokenResponse, error) {
	if len(oldToken) < 40 || len(oldToken) > 128 {
		return RotateTokenResponse{}, ErrInvalidCredential
	}
	var raw [32]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return RotateTokenResponse{}, fmt.Errorf("generate rotated credential: %w", err)
	}
	newToken := base64.RawURLEncoding.EncodeToString(raw[:])
	now := s.now().UTC().UnixMilli()
	if err := s.repo.RotateCredential(ctx, deviceID, hashSecret(oldToken), hashSecret(newToken), now); err != nil {
		return RotateTokenResponse{}, err
	}
	return RotateTokenResponse{ProtocolVersion: ProtocolVersion, DeviceID: deviceID, DeviceToken: newToken, CreatedAtMS: now}, nil
}

func (s *Service) RevokeDevice(ctx context.Context, deviceID string) error {
	if !deviceIDPattern.MatchString(deviceID) {
		return ErrInvalidInput
	}
	return s.repo.RevokeDevice(ctx, deviceID, s.now().UTC().UnixMilli())
}

func normalizePairingCode(code string) string { return strings.ToUpper(strings.TrimSpace(code)) }

func validatePairRequest(req PairRequest) error {
	code := normalizePairingCode(req.PairingCode)
	if len(code) != 52 {
		return ErrInvalidPairingCode
	}
	if _, err := base32.StdEncoding.WithPadding(base32.NoPadding).DecodeString(code); err != nil {
		return ErrInvalidPairingCode
	}
	if !deviceIDPattern.MatchString(req.DeviceID) || !validDisplayName(req.DisplayName) {
		return ErrInvalidInput
	}
	if containsOS(req.OSName) > 3 {
		return ErrInvalidInput
	}
	if !safeVersionPattern.MatchString(req.AppVersion) {
		return ErrInvalidInput
	}
	if len(req.TimeZone) == 0 || len(req.TimeZone) > 64 {
		return ErrInvalidInput
	}
	if _, err := time.LoadLocation(req.TimeZone); err != nil {
		return ErrInvalidInput
	}
	return nil
}

func containsOS(value string) int {
	switch value {
	case "windows":
		return 0
	case "macos":
		return 1
	case "linux":
		return 2
	case "other":
		return 3
	default:
		return 4
	}
}

func validDisplayName(name string) bool {
	name = strings.TrimSpace(name)
	if !utf8.ValidString(name) || utf8.RuneCountInString(name) == 0 || utf8.RuneCountInString(name) > MaxDisplayName {
		return false
	}
	for _, r := range name {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}

func (s *Service) ApplyUsageBatch(ctx context.Context, deviceID string, records []UsageEnvelope) ([]UsageItemResult, error) {
	if !deviceIDPattern.MatchString(deviceID) || len(records) == 0 || len(records) > 200 {
		return nil, ErrInvalidInput
	}
	seen := make(map[string]struct{}, len(records))
	for _, e := range records {
		if err := validateUsageEnvelope(deviceID, e); err != nil {
			return nil, err
		}
		if _, ok := seen[e.Record.RecordID]; ok {
			return nil, ErrInvalidInput
		}
		seen[e.Record.RecordID] = struct{}{}
	}
	return s.repo.ApplyUsageBatch(ctx, deviceID, records, s.now().UTC().UnixMilli())
}

func validateUsageEnvelope(deviceID string, e UsageEnvelope) error {
	r := e.Record
	if r.OwnerDeviceID != deviceID || len(r.RecordID) != 64 || !regexp.MustCompile(`^[0-9a-f]{64}$`).MatchString(r.RecordID) {
		return ErrInvalidInput
	}
	if !safeVersionPattern.MatchString(r.ToolID) || len(r.ToolID) > 64 || !strings.Contains("|codex|claude-code|opencode|antigravity|deepseek-harness|pi|", "|"+r.ToolID+"|") {
		return ErrInvalidInput
	}
	if r.Scope != "device_local" && r.Scope != "account_global" && r.Scope != "unknown" {
		return ErrInvalidInput
	}
	if r.Precision != "exact" && r.Precision != "aggregate" && r.Precision != "estimated" && r.Precision != "unknown" {
		return ErrInvalidInput
	}
	if r.Scope == "account_global" && (r.AccountID == nil || !validOpaque(*r.AccountID, "acct_")) {
		return ErrInvalidInput
	}
	if r.AccountID != nil && !validOpaque(*r.AccountID, "acct_") {
		return ErrInvalidInput
	}
	if r.ModelID != nil && !validUsageText(*r.ModelID, 256) {
		return ErrInvalidInput
	}
	if r.PriceVersion != nil && !validUsageText(*r.PriceVersion, 128) {
		return ErrInvalidInput
	}
	if r.ProjectID != nil && !validOpaque(*r.ProjectID, "prj_") {
		return ErrInvalidInput
	}
	if r.SessionID != nil && !validOpaque(*r.SessionID, "sess_") {
		return ErrInvalidInput
	}
	if r.EstimatedCostMicroUSD != nil && *r.EstimatedCostMicroUSD < 0 {
		return ErrInvalidInput
	}
	if r.IsDeleted && (r.Usage.InputUncachedTokens != 0 || r.Usage.CacheReadTokens != 0 || r.Usage.CacheWriteTokens != 0 || r.Usage.OutputTokens != 0 || r.Usage.UnclassifiedTokens != 0 || r.Usage.ReasoningTokens != nil || r.EstimatedCostMicroUSD != nil) {
		return ErrInvalidInput
	}
	if r.Precision == "exact" && r.Usage.UnclassifiedTokens != 0 {
		return ErrInvalidInput
	}
	if r.Usage.InputUncachedTokens > ^uint64(0)-r.Usage.CacheReadTokens || r.Usage.InputUncachedTokens+r.Usage.CacheReadTokens > ^uint64(0)-r.Usage.CacheWriteTokens || r.Usage.InputUncachedTokens+r.Usage.CacheReadTokens+r.Usage.CacheWriteTokens > ^uint64(0)-r.Usage.OutputTokens || r.Usage.InputUncachedTokens+r.Usage.CacheReadTokens+r.Usage.CacheWriteTokens+r.Usage.OutputTokens > ^uint64(0)-r.Usage.UnclassifiedTokens {
		return ErrInvalidInput
	}
	if r.Usage.ReasoningTokens != nil && *r.Usage.ReasoningTokens > r.Usage.OutputTokens {
		return ErrInvalidInput
	}
	if r.Scope == "unknown" && r.OwnerDeviceID != deviceID {
		return ErrInvalidInput
	}
	if e.RecordRevision == 0 || e.RecordRevision > uint64(1<<63-1) || len(e.PayloadHash) != 64 || !regexp.MustCompile(`^[0-9a-f]{64}$`).MatchString(e.PayloadHash) {
		return ErrInvalidInput
	}
	// Hash exactly the Rust serde field order, including explicit nulls.
	raw, err := canonicalUsageJSON(r)
	if err != nil || len(raw) > 65536 {
		return ErrInvalidInput
	}
	sum := sha256.Sum256(append([]byte("tokenscope/canonical-record/v1\x00"), raw...))
	if hex.EncodeToString(sum[:]) != e.PayloadHash {
		return ErrPayloadHashMismatch
	}
	return nil
}

func validUsageText(value string, max int) bool {
	if len(value) == 0 || len(value) > max {
		return false
	}
	for _, r := range value {
		if unicode.IsControl(r) || r == '\u2028' || r == '\u2029' {
			return false
		}
	}
	return true
}

func validOpaque(value, prefix string) bool {
	if !strings.HasPrefix(value, prefix) || len(value) != len(prefix)+64 {
		return false
	}
	for _, c := range value[len(prefix):] {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}

func canonicalUsageJSON(r UsageRecord) ([]byte, error) {
	// encoding/json escapes HTML by default; disable it to match serde_json.
	var b strings.Builder
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(r); err != nil {
		return nil, err
	}
	return []byte(strings.TrimSuffix(b.String(), "\n")), nil
}

// PullUsageChanges returns a bounded page from the Hub-wide paired-device workspace.
// All devices paired to one Hub share this workspace; multi-tenant isolation is not
// implied by device authentication and must be added before serving unrelated users.
func (s *Service) PullUsageChanges(ctx context.Context, afterSeq int64, limit int) (UsageChangesResponse, error) {
	if afterSeq < 0 || limit < 1 || limit > 500 {
		return UsageChangesResponse{}, ErrInvalidInput
	}
	changes, high, err := s.repo.PullUsageChanges(ctx, afterSeq, limit)
	if err != nil {
		return UsageChangesResponse{}, err
	}
	// Keep response memory and wire size bounded even if records contain unusually
	// large metadata. Cursor advances only through entries actually returned.
	for len(changes) > 0 {
		next := afterSeq
		for _, change := range changes {
			if change.ChangeSeq > next {
				next = change.ChangeSeq
			}
		}
		response := UsageChangesResponse{ProtocolVersion: ProtocolVersion, Changes: changes, NextCursor: next, HighWatermark: high, HasMore: next < high}
		encoded, marshalErr := json.Marshal(response)
		if marshalErr != nil {
			return UsageChangesResponse{}, marshalErr
		}
		if len(encoded) <= 2*1024*1024 {
			return response, nil
		}
		changes = changes[:len(changes)-1]
	}
	return UsageChangesResponse{ProtocolVersion: ProtocolVersion, Changes: []UsageChange{}, NextCursor: afterSeq, HighWatermark: high, HasMore: afterSeq < high}, nil
}
