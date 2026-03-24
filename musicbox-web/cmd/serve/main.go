package main

import (
	"log/slog"
	"net/http"
	"os"
)

func main() {
	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}

	dir := os.Getenv("MUSICBOX_WEB_DIR")
	if dir == "" {
		dir = "www"
	}

	slog.Info("serving musicbox", "dir", dir, "port", port)
	if err := http.ListenAndServe(":"+port, http.FileServer(http.Dir(dir))); err != nil {
		slog.Error("server failed", "err", err)
	}
}
