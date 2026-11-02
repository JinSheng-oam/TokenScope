package httpapi

import (
	"context"
	"net/http"

	"example.com/tokenscope/hub/internal/hub"
)

func contextWithDevice(r *http.Request, d hub.Device) context.Context {
	return context.WithValue(r.Context(), deviceContextKey{}, d)
}
func deviceFromContext(r *http.Request) hub.Device {
	d, _ := r.Context().Value(deviceContextKey{}).(hub.Device)
	return d
}
