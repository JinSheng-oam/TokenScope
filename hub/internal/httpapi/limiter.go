package httpapi

import (
	"sync"
	"time"
)

type bucket struct {
	start time.Time
	count int
}
type pairLimiter struct {
	mu      sync.Mutex
	buckets map[string]bucket
	limit   int
	window  time.Duration
}

func newPairLimiter() *pairLimiter {
	return &pairLimiter{buckets: map[string]bucket{}, limit: 10, window: time.Minute}
}
func (l *pairLimiter) Allow(key string) bool {
	now := time.Now()
	l.mu.Lock()
	defer l.mu.Unlock()
	if len(l.buckets) > 4096 {
		for k, b := range l.buckets {
			if now.Sub(b.start) > 2*l.window {
				delete(l.buckets, k)
			}
		}
	}
	b := l.buckets[key]
	if b.start.IsZero() || now.Sub(b.start) >= l.window {
		b = bucket{start: now}
	}
	if b.count >= l.limit {
		l.buckets[key] = b
		return false
	}
	b.count++
	l.buckets[key] = b
	return true
}
