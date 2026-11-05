package hub

import (
	"errors"
	"fmt"
	"time"
)

const (
	ProtocolVersion = 1
	PairingCodeTTL  = 5 * time.Minute
	MaxDisplayName  = 64
)

var (
	ErrInvalidPairingCode  = errors.New("invalid or expired pairing code")
	ErrDeviceExists        = errors.New("device already registered")
	ErrInvalidCredential   = errors.New("invalid device credential")
	ErrDeviceRevoked       = errors.New("device revoked")
	ErrDeviceNotFound      = errors.New("device not found")
	ErrCredentialConflict  = errors.New("credential already exists")
	ErrInvalidInput        = errors.New("invalid input")
	ErrPayloadHashMismatch = errors.New("payload hash mismatch")
	ErrRevisionConflict    = errors.New("same revision has different payload")
)

type Device struct {
	DeviceID    string `json:"device_id"`
	DisplayName string `json:"display_name"`
	OSName      string `json:"os_name"`
	AppVersion  string `json:"app_version"`
	TimeZone    string `json:"time_zone"`
	CreatedAtMS int64  `json:"created_at_ms"`
	LastSeenMS  *int64 `json:"last_seen_ms"`
	RevokedAtMS *int64 `json:"revoked_at_ms"`
}

func (d Device) Revoked() bool { return d.RevokedAtMS != nil }

type PairRequest struct {
	PairingCode string `json:"pairing_code"`
	DeviceID    string `json:"device_id"`
	DisplayName string `json:"display_name"`
	OSName      string `json:"os_name"`
	AppVersion  string `json:"app_version"`
	TimeZone    string `json:"time_zone"`
}

type PairResponse struct {
	ProtocolVersion int    `json:"protocol_version"`
	DeviceID        string `json:"device_id"`
	DeviceToken     string `json:"device_token"`
	CreatedAtMS     int64  `json:"created_at_ms"`
}

type PairingCodeResponse struct {
	PairingCode string `json:"pairing_code"`
	ExpiresAtMS int64  `json:"expires_at_ms"`
}

type DeviceListResponse struct {
	ProtocolVersion int      `json:"protocol_version"`
	Devices         []Device `json:"devices"`
}

type RotateTokenResponse struct {
	ProtocolVersion int    `json:"protocol_version"`
	DeviceID        string `json:"device_id"`
	DeviceToken     string `json:"device_token"`
	CreatedAtMS     int64  `json:"created_at_ms"`
}

// String methods redact one-time secrets to reduce accidental log disclosure.
func (r PairRequest) String() string {
	return fmt.Sprintf("PairRequest{device_id:%q, display_name:%q, os_name:%q, app_version:%q, time_zone:%q, pairing_code:[REDACTED]}", r.DeviceID, r.DisplayName, r.OSName, r.AppVersion, r.TimeZone)
}

func (r PairResponse) String() string {
	return fmt.Sprintf("PairResponse{protocol_version:%d, device_id:%q, device_token:[REDACTED], created_at_ms:%d}", r.ProtocolVersion, r.DeviceID, r.CreatedAtMS)
}

func (r PairingCodeResponse) String() string {
	return fmt.Sprintf("PairingCodeResponse{pairing_code:[REDACTED], expires_at_ms:%d}", r.ExpiresAtMS)
}

// UsageEnvelope is the versioned, canonical event payload sent by one paired device.
// Field order is kept aligned with Rust CanonicalRecord for cross-language hashing.
type UsageBreakdown struct {
	InputUncachedTokens uint64  `json:"input_uncached_tokens"`
	CacheReadTokens     uint64  `json:"cache_read_tokens"`
	CacheWriteTokens    uint64  `json:"cache_write_tokens"`
	OutputTokens        uint64  `json:"output_tokens"`
	UnclassifiedTokens  uint64  `json:"unclassified_tokens"`
	ReasoningTokens     *uint64 `json:"reasoning_tokens"`
}
type UsageRecord struct {
	RecordID              string         `json:"record_id"`
	OwnerDeviceID         string         `json:"owner_device_id"`
	ToolID                string         `json:"tool_id"`
	Scope                 string         `json:"scope"`
	Precision             string         `json:"precision"`
	AccountID             *string        `json:"account_id"`
	ModelID               *string        `json:"model_id"`
	ProjectID             *string        `json:"project_id"`
	SessionID             *string        `json:"session_id"`
	EventAtMS             *int64         `json:"event_at_ms"`
	Usage                 UsageBreakdown `json:"usage"`
	EstimatedCostMicroUSD *int64         `json:"estimated_cost_microusd"`
	PriceVersion          *string        `json:"price_version"`
	IsDeleted             bool           `json:"is_deleted"`
}
type UsageEnvelope struct {
	Record         UsageRecord `json:"record"`
	RecordRevision uint64      `json:"record_revision"`
	PayloadHash    string      `json:"payload_hash"`
}
type ProjectNameInput struct {
	ProjectID string `json:"project_id"`
	Name      string `json:"name"`
}
type ProjectName struct {
	OwnerDeviceID string `json:"owner_device_id"`
	ProjectID     string `json:"project_id"`
	Name          string `json:"name"`
	UpdatedAtMS   int64  `json:"updated_at_ms"`
}
type ProjectNamesRequest struct {
	ProtocolVersion int                `json:"protocol_version"`
	ProjectNames    []ProjectNameInput `json:"project_names"`
}
type ProjectNamesUpsertResponse struct {
	ProtocolVersion int `json:"protocol_version"`
	Accepted        int `json:"accepted"`
}
type ProjectNamesPage struct {
	ProtocolVersion int           `json:"protocol_version"`
	ProjectNames    []ProjectName `json:"project_names"`
	NextDeviceID    string        `json:"next_device_id"`
	NextProjectID   string        `json:"next_project_id"`
	HasMore         bool          `json:"has_more"`
}
type UsageBatchRequest struct {
	ProtocolVersion int             `json:"protocol_version"`
	BatchID         string          `json:"batch_id"`
	Records         []UsageEnvelope `json:"records"`
}
type UsageItemResult struct {
	RecordID       string `json:"record_id"`
	RecordRevision uint64 `json:"record_revision"`
	Status         string `json:"status"` // accepted, unchanged, superseded
	HubRevision    uint64 `json:"hub_revision"`
}
type UsageBatchResponse struct {
	ProtocolVersion int               `json:"protocol_version"`
	BatchID         string            `json:"batch_id"`
	Results         []UsageItemResult `json:"results"`
}

// UsageChange is an immutable change-feed entry. Sequence is global to this Hub
// workspace; the payload always contains the latest accepted revision at this sequence.
type UsageChange struct {
	ChangeSeq     int64         `json:"change_seq"`
	OwnerDeviceID string        `json:"owner_device_id"`
	Envelope      UsageEnvelope `json:"envelope"`
	ChangedAtMS   int64         `json:"changed_at_ms"`
}
type UsageChangesResponse struct {
	ProtocolVersion int           `json:"protocol_version"`
	Changes         []UsageChange `json:"changes"`
	NextCursor      int64         `json:"next_cursor"`
	HighWatermark   int64         `json:"high_watermark"`
	HasMore         bool          `json:"has_more"`
}
