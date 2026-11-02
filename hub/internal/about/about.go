// Package about reports the deliberately unimplemented Hub bootstrap state.
package about

const Version = "0.1.0"
const Stage = "M0-01"

func Banner() string {
	return "TokenScope Hub " + Version + " scaffold (" + Stage + "); HTTP and sync not implemented"
}
