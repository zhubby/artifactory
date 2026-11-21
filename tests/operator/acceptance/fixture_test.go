// [INPUT]: 真实 TLS HTTP 服务器、LFS、镜像规划与双架构源码夹具。
// [OUTPUT]: 验证同源源码、构建镜像目标和真实 RUN 夹具。
// [POS]: 规划上游的公开契约回归，不连接 Kubernetes。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
package main

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
)

func TestImagePlatformsAndExplicitEmulation(t *testing.T) {
	for _, invalid := range []string{"linux/ppc64le", "linux/amd64,linux/amd64", "linux/amd64,linux/arm64,linux/amd64", "linux/arm64,"} {
		if _, err := imagePlatforms(invalid, "arm64"); err == nil {
			t.Fatalf("accepted invalid platform %q", invalid)
		}
	}
	platforms, err := imagePlatforms("linux/amd64,linux/arm64", "amd64")
	if err != nil {
		t.Fatal(err)
	}
	for _, allow := range []bool{false, true} {
		target := imageTarget("amd64", platforms, allow, 5)
		image := target["image"].(map[string]any)
		if target["executor"] != "buildkit" || image["allow_emulation"] != allow || len(image["platforms"].([]string)) != 2 {
			t.Fatalf("image contract lost explicit settings: %+v", target)
		}
		if image["name"] != "image" || len(image["tags"].([]string)) != 0 {
			t.Fatalf("fixture overrides build target identity or Release version: %+v", image)
		}
	}
}

func TestImageRepositoryRequiresBothArchitectureExecutables(t *testing.T) {
	repo, binaries := t.TempDir(), t.TempDir()
	if err := installImageSource(repo, binaries); err == nil {
		t.Fatal("missing executable accepted")
	}
	for _, arch := range []string{"arm64", "amd64"} {
		if err := os.WriteFile(filepath.Join(binaries, "arch-check-"+arch), []byte("fixture-"+arch), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := installImageSource(repo, binaries); err != nil {
		t.Fatal(err)
	}
	dockerfile, err := os.ReadFile(filepath.Join(repo, "Dockerfile"))
	if err != nil || !strings.Contains(string(dockerfile), "RUN [\"/arch-check\"]") || !strings.Contains(string(dockerfile), "arch-check-${TARGETARCH}") {
		t.Fatalf("repository does not prove architecture execution: %q %v", dockerfile, err)
	}
	for _, arch := range []string{"arm64", "amd64"} {
		info, err := os.Stat(filepath.Join(repo, "arch-check-"+arch))
		if err != nil || info.Mode().Perm()&0100 == 0 {
			t.Fatalf("architecture executable is not executable: %v", err)
		}
	}
}

func TestLFSDownloadUsesRequestOrigin(t *testing.T) {
	payload := []byte("source-from-upstream\n")
	oid := fmt.Sprintf("%x", sha256.Sum256(payload))
	var down atomic.Bool
	mux := http.NewServeMux()
	mux.HandleFunc("/repo.git/info/lfs/objects/batch", lfsBatch(oid, payload, &down))
	mux.HandleFunc("/repo.git/lfs/objects/"+oid, func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write(payload)
	})
	server := httptest.NewTLSServer(mux)
	defer server.Close()
	origin, _ := url.Parse(server.URL)
	for _, host := range []string{origin.Host, "upstream:9443", "192.0.2.3:30443"} {
		t.Run(host, func(t *testing.T) {
			body := fmt.Sprintf(`{"objects":[{"oid":%q,"size":%d}]}`, oid, len(payload))
			request, err := http.NewRequest(http.MethodPost, server.URL+"/repo.git/info/lfs/objects/batch", strings.NewReader(body))
			if err != nil {
				t.Fatal(err)
			}
			request.Host = host
			request.Header.Set("Forwarded", "host=wrong.invalid;proto=http")
			request.Header.Set("X-Forwarded-Host", "wrong.invalid")
			response, err := server.Client().Do(request)
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			var result struct {
				Objects []struct {
					Actions struct {
						Download struct{ Href string }
					}
				}
			}
			if err := json.NewDecoder(response.Body).Decode(&result); err != nil {
				t.Fatal(err)
			}
			if len(result.Objects) != 1 {
				t.Fatalf("expected one LFS object: %+v", result)
			}
			href := result.Objects[0].Actions.Download.Href
			if want := "https://" + host + "/repo.git/lfs/objects/" + oid; href != want {
				t.Fatalf("download escaped request origin: got %q want %q", href, want)
			}
			if host == origin.Host {
				download, err := server.Client().Get(href)
				if err != nil {
					t.Fatal(err)
				}
				defer download.Body.Close()
				data, err := io.ReadAll(download.Body)
				if err != nil || string(data) != string(payload) {
					t.Fatalf("LFS download failed: %q, %v", data, err)
				}
			}
		})
	}
}
