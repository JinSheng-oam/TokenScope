package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"example.com/tokenscope/hub/internal/httpapi"
	"example.com/tokenscope/hub/internal/hub"
	"example.com/tokenscope/hub/internal/sqlstore"
	_ "modernc.org/sqlite"
)

func main() {
	args := os.Args[1:]
	if len(args) > 0 && args[0] == "pair-code" {
		if err := runPairCode(args[1:]); err != nil {
			log.Fatal(err)
		}
		return
	}
	if len(args) > 0 && args[0] == "serve" {
		args = args[1:]
	}
	if err := runServe(args); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
}

func runPairCode(args []string) error {
	fs := flag.NewFlagSet("pair-code", flag.ContinueOnError)
	dbPath := fs.String("db", "./data/tokenscope-hub.db", "Hub SQLite database path")
	if err := fs.Parse(args); err != nil {
		return err
	}
	repo, err := sqlstore.Open(*dbPath)
	if err != nil {
		return err
	}
	defer repo.Close()
	result, err := hub.NewService(repo).CreatePairingCode(context.Background())
	if err != nil {
		return err
	}
	fmt.Printf("Pairing code (one-time display): %s\nExpires at (UTC ms): %d\n", result.PairingCode, result.ExpiresAtMS)
	return nil
}

func runServe(args []string) error {
	fs := flag.NewFlagSet("serve", flag.ContinueOnError)
	dbPath := fs.String("db", "./data/tokenscope-hub.db", "Hub SQLite database path")
	listen := fs.String("listen", "127.0.0.1:8787", "HTTP listen address; expose only behind trusted HTTPS reverse proxy")
	tlsCert := fs.String("tls-cert", "", "TLS certificate (optional when behind reverse proxy)")
	tlsKey := fs.String("tls-key", "", "TLS private key (optional when behind reverse proxy)")
	if err := fs.Parse(args); err != nil {
		return err
	}
	adminToken := os.Getenv("TOKENSCOPE_ADMIN_TOKEN")
	if len(adminToken) < 32 {
		return errors.New("TOKENSCOPE_ADMIN_TOKEN is required and must contain at least 32 characters")
	}
	trusted := splitCSV(os.Getenv("TOKENSCOPE_TRUSTED_PROXY_CIDRS"))
	host, _, err := net.SplitHostPort(*listen)
	if err != nil {
		return fmt.Errorf("invalid listen address: %w", err)
	}
	ip := net.ParseIP(host)
	if ip == nil || !ip.IsLoopback() {
		if len(trusted) == 0 && (*tlsCert == "" || *tlsKey == "") {
			return errors.New("non-loopback bind requires direct TLS or TOKENSCOPE_TRUSTED_PROXY_CIDRS")
		}
	}
	if (*tlsCert == "") != (*tlsKey == "") {
		return errors.New("both --tls-cert and --tls-key must be provided")
	}
	repo, err := sqlstore.Open(*dbPath)
	if err != nil {
		return err
	}
	defer repo.Close()
	handler, err := httpapi.NewHandler(repo, httpapi.Config{AdminToken: adminToken, TrustedProxyCIDRs: trusted})
	if err != nil {
		return err
	}
	server := &http.Server{Addr: *listen, Handler: handler, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 15 * time.Second, WriteTimeout: 30 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 16 * 1024}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	serveErr := make(chan error, 1)
	go func() {
		if *tlsCert != "" {
			serveErr <- server.ListenAndServeTLS(*tlsCert, *tlsKey)
		} else {
			serveErr <- server.ListenAndServe()
		}
	}()
	select {
	case err := <-serveErr:
		return err
	case <-ctx.Done():
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		return server.Shutdown(shutdownCtx)
	}
}
func splitCSV(v string) []string {
	if strings.TrimSpace(v) == "" {
		return nil
	}
	parts := strings.Split(v, ",")
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if strings.TrimSpace(p) != "" {
			out = append(out, strings.TrimSpace(p))
		}
	}
	return out
}
