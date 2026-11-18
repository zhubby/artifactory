// [INPUT]: Go 运行架构、受限构建延迟与可写镜像根目录。
// [OUTPUT]: 写入实际执行架构，证明 Dockerfile RUN 真正执行。
// [POS]: 验收夹具的无网络静态程序，分别交叉编译到 amd64/arm64。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
package main

import (
	"fmt"
	"os"
	"runtime"
	"strconv"
	"time"
)

func main() {
	if raw := os.Getenv("ARCH_CHECK_DELAY_SECONDS"); raw != "" {
		seconds, err := strconv.Atoi(raw)
		if err != nil || seconds < 0 || seconds > 30 {
			panic("invalid architecture check delay")
		}
		time.Sleep(time.Duration(seconds) * time.Second)
	}
	marker := "executed-" + runtime.GOOS + "-" + runtime.GOARCH + "\n"
	if err := os.WriteFile("/arch-result", []byte(marker), 0644); err != nil {
		panic(err)
	}
	fmt.Print(marker)
}
