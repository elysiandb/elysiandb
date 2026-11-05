package cmd

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"mime"
	"net/http"
	"net/http/cookiejar"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/backup"
	"github.com/taymour/elysiandb/internal/durable"
)

const RemotePasswordEnv = "ELYSIANDB_PASSWORD"

type remoteBackup struct {
	url      string
	user     string
	password string
	token    string
	output   string
	verify   bool
}

func RunsWithoutStorage(args []string) bool {
	if len(args) == 0 || args[0] != BackupCommand {
		return false
	}

	for _, arg := range args[1:] {
		if arg == "--url" || arg == "-url" || strings.HasPrefix(arg, "--url=") {
			return true
		}
	}

	return false
}

func (r *remoteBackup) run() (string, error) {
	jar, err := cookiejar.New(nil)
	if err != nil {
		return "", err
	}

	client := &http.Client{Jar: jar}
	base := strings.TrimRight(r.url, "/")
	basic := false

	if r.user != "" {
		if r.password == "" {
			r.password = os.Getenv(RemotePasswordEnv)
		}

		if r.password == "" {
			return "", fmt.Errorf("--user needs a password in the %s environment variable", RemotePasswordEnv)
		}

		body, _ := json.Marshal(map[string]string{"username": r.user, "password": r.password})
		resp, err := client.Post(base+"/api/security/login", "application/json", bytes.NewReader(body))
		if err != nil {
			return "", err
		}
		resp.Body.Close()

		switch resp.StatusCode {
		case http.StatusOK:
		case http.StatusUnauthorized, http.StatusForbidden, http.StatusTooManyRequests:
			if !r.basicIsPossible(client, base) {
				return "", fmt.Errorf("login refused: %s", resp.Status)
			}
			basic = true
		default:
			basic = true
		}
	}

	req, err := http.NewRequest(http.MethodGet, base+"/api/backup", nil)
	if err != nil {
		return "", err
	}

	if basic {
		req.SetBasicAuth(r.user, r.password)
	}

	if r.token != "" {
		req.Header.Set("Authorization", "Bearer "+r.token)
	}

	resp, err := client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		detail, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return "", fmt.Errorf("the server answered %s: %s", resp.Status, strings.TrimSpace(string(detail)))
	}

	name := backup.FileName(time.Now().UTC())
	if _, params, err := mime.ParseMediaType(resp.Header.Get("Content-Disposition")); err == nil && params["filename"] != "" {
		name = filepath.Base(params["filename"])
	}

	path := filepath.Join(r.output, name)
	if _, err := os.Stat(path); err == nil {
		return "", fmt.Errorf("%s already exists", path)
	}

	tmp, err := os.CreateTemp(r.output, "."+name+".*")
	if err != nil {
		return "", err
	}
	defer os.Remove(tmp.Name())
	defer tmp.Close()

	if _, err := io.Copy(tmp, resp.Body); err != nil {
		return "", fmt.Errorf("the backup was interrupted: %w", err)
	}

	if err := tmp.Sync(); err != nil {
		return "", err
	}

	if r.verify {
		if _, err := tmp.Seek(0, io.SeekStart); err != nil {
			return "", err
		}

		if _, err := backup.Verify(tmp); err != nil {
			return "", err
		}
	}

	if err := tmp.Close(); err != nil {
		return "", err
	}

	if err := os.Rename(tmp.Name(), path); err != nil {
		return "", err
	}

	if err := durable.OS.SyncDir(r.output); err != nil {
		return "", err
	}

	return path, nil
}

func (r *remoteBackup) basicIsPossible(client *http.Client, base string) bool {
	req, err := http.NewRequest(http.MethodHead, base+"/api/backup", nil)
	if err != nil {
		return false
	}

	req.SetBasicAuth(r.user, r.password)
	resp, err := client.Do(req)
	if err != nil {
		return false
	}
	resp.Body.Close()

	return resp.StatusCode != http.StatusUnauthorized && resp.StatusCode != http.StatusForbidden
}

func parseRemoteArgs(args []string) (*remoteBackup, error) {
	r := &remoteBackup{output: ".", verify: true}
	value := func(arg string, i *int) (string, error) {
		if name, v, ok := strings.Cut(arg, "="); ok && strings.HasPrefix(name, "-") {
			return v, nil
		}

		if *i+1 >= len(args) {
			return "", fmt.Errorf("%s needs a value", arg)
		}

		*i++

		return args[*i], nil
	}

	for i := 0; i < len(args); i++ {
		arg := args[i]
		name, _, _ := strings.Cut(arg, "=")

		var err error
		switch name {
		case "--url", "-url":
			r.url, err = value(arg, &i)
		case "--user", "-user":
			r.user, err = value(arg, &i)
		case "--token", "-token":
			r.token, err = value(arg, &i)
		case "--output", "-output", "-o":
			r.output, err = value(arg, &i)
		case "--no-verify":
			r.verify = false
		default:
			return nil, fmt.Errorf("unknown argument %q, usage: elysiandb backup [--output <folder>] [--url <address> [--user <name>] [--token <token>] [--no-verify]]", arg)
		}

		if err != nil {
			return nil, err
		}
	}

	if r.url == "" {
		return nil, errors.New("--url is empty")
	}

	return r, nil
}
