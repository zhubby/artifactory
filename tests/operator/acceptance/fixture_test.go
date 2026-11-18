// [INPUT]: 真实 TLS HTTP 服务器、LFS batch 夹具与多种请求地址。
// [OUTPUT]: 验证下载地址保留请求源地址，忽略转发头。
// [POS]: 阻止集群 Service 地址泄漏到主机 NodePort 的夹具回归。
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
	"strings"
	"sync/atomic"
	"testing"
)

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
