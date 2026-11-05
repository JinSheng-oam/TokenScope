package hub

import "context"

// Repository methods that consume a pairing code or rotate a credential must
// be atomic. Implementations must never return raw pairing codes or tokens.
type Repository interface {
	CreatePairingCode(ctx context.Context, codeHash string, createdAtMS, expiresAtMS int64) error
	PairDevice(ctx context.Context, codeHash string, nowMS int64, device Device, credentialHash string) error
	Authenticate(ctx context.Context, credentialHash string, nowMS int64) (Device, error)
	ListDevices(ctx context.Context) ([]Device, error)
	RenameDevice(ctx context.Context, deviceID, displayName string) error
	RotateCredential(ctx context.Context, deviceID, oldHash, newHash string, nowMS int64) error
	RevokeDevice(ctx context.Context, deviceID string, nowMS int64) error
	ApplyUsageBatch(ctx context.Context, deviceID string, records []UsageEnvelope, nowMS int64) ([]UsageItemResult, error)
	PullUsageChanges(ctx context.Context, afterSeq int64, limit int) ([]UsageChange, int64, error)
	UpsertProjectNames(ctx context.Context, deviceID string, names []ProjectName, nowMS int64) error
	ListProjectNames(ctx context.Context, afterDeviceID, afterProjectID string, limit int) ([]ProjectName, bool, error)
	Close() error
}
