package about

import (
	"strings"
	"testing"
)

func TestBannerStatesNotImplemented(t *testing.T) {
	banner := Banner()
	if !strings.Contains(banner, "not implemented") {
		t.Fatalf("must not imply Hub is operational: %s", banner)
	}
}
