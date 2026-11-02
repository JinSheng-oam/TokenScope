package hub

import (
	"context"
	"sort"
	"sync"
)

type pairingCode struct {
	expiresAtMS  int64
	consumedAtMS *int64
}
type credential struct {
	deviceID    string
	revokedAtMS *int64
}

// MemoryRepository is used by deterministic unit tests only. Production Hub
// must use sqlstore.SQLiteRepository.
type MemoryRepository struct {
	mu          sync.Mutex
	codes       map[string]pairingCode
	devices     map[string]Device
	credentials map[string]credential
	usage       map[string]UsageEnvelope
	feed        []UsageChange
	nextSeq     int64
}

func NewMemoryRepository() *MemoryRepository {
	return &MemoryRepository{codes: map[string]pairingCode{}, devices: map[string]Device{}, credentials: map[string]credential{}, usage: map[string]UsageEnvelope{}, nextSeq: 1}
}
func (m *MemoryRepository) CreatePairingCode(_ context.Context, hash string, _, expires int64) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, ok := m.codes[hash]; ok {
		return ErrCredentialConflict
	}
	m.codes[hash] = pairingCode{expiresAtMS: expires}
	return nil
}
func (m *MemoryRepository) PairDevice(_ context.Context, codeHash string, now int64, d Device, credentialHash string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	c, ok := m.codes[codeHash]
	if !ok || c.consumedAtMS != nil || c.expiresAtMS <= now {
		return ErrInvalidPairingCode
	}
	if _, exists := m.devices[d.DeviceID]; exists {
		return ErrDeviceExists
	}
	if _, exists := m.credentials[credentialHash]; exists {
		return ErrCredentialConflict
	}
	consumed := now
	c.consumedAtMS = &consumed
	m.codes[codeHash] = c
	m.devices[d.DeviceID] = d
	m.credentials[credentialHash] = credential{deviceID: d.DeviceID}
	return nil
}
func (m *MemoryRepository) Authenticate(_ context.Context, hash string, now int64) (Device, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	c, ok := m.credentials[hash]
	if !ok {
		return Device{}, ErrInvalidCredential
	}
	d, ok := m.devices[c.deviceID]
	if !ok {
		return Device{}, ErrInvalidCredential
	}
	if d.Revoked() {
		return Device{}, ErrDeviceRevoked
	}
	if c.revokedAtMS != nil {
		return Device{}, ErrInvalidCredential
	}
	d.LastSeenMS = &now
	m.devices[d.DeviceID] = d
	return d, nil
}
func (m *MemoryRepository) ListDevices(_ context.Context) ([]Device, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([]Device, 0, len(m.devices))
	for _, d := range m.devices {
		out = append(out, d)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].CreatedAtMS < out[j].CreatedAtMS })
	return out, nil
}
func (m *MemoryRepository) RenameDevice(_ context.Context, id, name string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	d, ok := m.devices[id]
	if !ok {
		return ErrDeviceNotFound
	}
	if d.Revoked() {
		return ErrDeviceRevoked
	}
	d.DisplayName = name
	m.devices[id] = d
	return nil
}
func (m *MemoryRepository) RotateCredential(_ context.Context, id, oldHash, newHash string, now int64) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	old, ok := m.credentials[oldHash]
	if !ok || old.deviceID != id || old.revokedAtMS != nil {
		return ErrInvalidCredential
	}
	d, ok := m.devices[id]
	if !ok {
		return ErrDeviceNotFound
	}
	if d.Revoked() {
		return ErrDeviceRevoked
	}
	if _, exists := m.credentials[newHash]; exists {
		return ErrCredentialConflict
	}
	old.revokedAtMS = &now
	m.credentials[oldHash] = old
	m.credentials[newHash] = credential{deviceID: id}
	return nil
}
func (m *MemoryRepository) RevokeDevice(_ context.Context, id string, now int64) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	d, ok := m.devices[id]
	if !ok {
		return ErrDeviceNotFound
	}
	if d.Revoked() {
		return nil
	}
	d.RevokedAtMS = &now
	m.devices[id] = d
	for h, c := range m.credentials {
		if c.deviceID == id && c.revokedAtMS == nil {
			t := now
			c.revokedAtMS = &t
			m.credentials[h] = c
		}
	}
	return nil
}
func (m *MemoryRepository) Close() error { return nil }

func (m *MemoryRepository) ApplyUsageBatch(_ context.Context, deviceID string, records []UsageEnvelope, nowMS int64) ([]UsageItemResult, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	d, ok := m.devices[deviceID]
	if !ok {
		return nil, ErrDeviceNotFound
	}
	if d.Revoked() {
		return nil, ErrDeviceRevoked
	}
	out := make([]UsageItemResult, 0, len(records))
	accepted := make([]UsageEnvelope, 0, len(records))
	for _, e := range records {
		key := deviceID + ":" + e.Record.RecordID
		old, exists := m.usage[key]
		status := "accepted"
		hubRev := e.RecordRevision
		if exists {
			hubRev = old.RecordRevision
			switch {
			case e.RecordRevision < old.RecordRevision:
				status = "superseded"
			case e.RecordRevision == old.RecordRevision:
				if e.PayloadHash != old.PayloadHash {
					return nil, ErrRevisionConflict
				}
				status = "unchanged"
			}
		}
		if status == "accepted" {
			accepted = append(accepted, e)
			hubRev = e.RecordRevision
		}
		out = append(out, UsageItemResult{RecordID: e.Record.RecordID, RecordRevision: e.RecordRevision, Status: status, HubRevision: hubRev})
	}
	for _, e := range accepted {
		m.usage[deviceID+":"+e.Record.RecordID] = e
		m.feed = append(m.feed, UsageChange{ChangeSeq: m.nextSeq, OwnerDeviceID: deviceID, Envelope: e, ChangedAtMS: nowMS})
		m.nextSeq++
	}
	return out, nil
}

func (m *MemoryRepository) PullUsageChanges(_ context.Context, afterSeq int64, limit int) ([]UsageChange, int64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if afterSeq < 0 || limit < 1 || limit > 500 {
		return nil, 0, ErrInvalidInput
	}
	high := m.nextSeq - 1
	out := make([]UsageChange, 0, limit)
	for _, c := range m.feed {
		if c.ChangeSeq > afterSeq {
			out = append(out, c)
			if len(out) == limit {
				break
			}
		}
	}
	return out, high, nil
}
