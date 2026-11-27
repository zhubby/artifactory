<!--
[INPUT]: 固定 0.2.6 源码、镜像和集群验收回执。
[OUTPUT]: 记录 Prism 控制台发布与现场验证边界。
[POS]: 0.2.6 部署证据；此文档晚于不可变发行源码。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Prism 0.2.6 Kubernetes 上线记录 · 2026-11-27

本轮发布 Prism 液态玻璃控制台和设计交付。
中文界面使用「工作空间」。
工作空间包含项目，项目包含版本和构建。
设计规范、120 项组件和交互原型随源码发行。

## 代码审查与验证

发布源码为 `fdf406f60da5cec22fff66a49c6ed01bd796fe56`。
不可变标签为 `v0.2.6`。
[发行页面](https://github.com/zhubby/artifactory/releases/tag/v0.2.6)提供四个 Helm 包及校验文件。
版本、源码提交和全部包校验和独立读回通过。

代码审查修复文档站点的三个设计入口。
原相对路径越出 mdBook，现在指向源码文档。
部署脚本加入预演摘要封存和备份摘要复核。
参数改动在集群写入前被拒绝的测试通过。

类型检查和生产构建通过。
版本契约测试 15 项通过。
完整 UI 回归 1,234 项通过、8 项条件跳过。
本轮完整回归没有失败或不稳定用例。
文档浏览器测试 14 项通过。
发行源码的文档检查覆盖 132 章和 24,023 个本地链接及资源。

## 镜像与发行恢复

10 个组件均验证 Linux AMD64 和 ARM64 清单。
两个平台的镜像标签指向同一发行源码与版本。
部署使用 AMD64 叶清单，复制保留原始摘要。
仓库为 `registry.kubeease.cn/pre-release`。
不可变来源标签为 `git-fdf406f60da5cec2`。

| 组件 | SHA-256 |
| --- | --- |
| Control / Telemetry | `dc4e68f7a9719638889121ba81f99a8b847e66381a2db52d215c5caf4608af73` |
| Web | `32e3d497eed56b3d42dec5334cdcaff665198eb085bb489e7cd1897a31dd9030` |
| Gateway | `690e0c92c068e3235639f2ebb546d7837b39a4ce0246f15544de7c4338e58cc3` |
| Planning Operator | `aec43c5e2d6dc129cba6c4c6f24aec15632d180037842a3ba209dd6fe2fe4081` |
| Planning Worker | `c4b64ffd3f166b3e9807d39ffe87f565d6f2bb068506bf513138e0b7b846298a` |
| Source Preparer | `4bda2a720ab58c7afdce79ab1ec0a6d7c3fb8de6a05dd09537506f4d79edc2d7` |
| Image Operator | `0c68c36bc6a2a2a8025518ddabc7d778f80c02e819899871927178e8a58f0007` |
| Image Worker | `bede4b1a99b8850607f8b6decda35710a66951bfbffdc25ee4e508e68ea3eee5` |
| BuildKit | `0fd1a8830577795005956e1824b53bb196cb4be54a027045a0d3239f958b7700` |

[镜像流水线](https://github.com/zhubby/artifactory/actions/runs/45996194782)中的 Planning Worker 构建成功，推送返回 `permission_denied: read_package`。
既有私有包没有关联仓库，阻塞流水线的后续清单任务。
恢复过程没有修改包的访问权限或可见性。
Planning Worker 使用固定源码、锁定依赖和固定基础摘要补发双架构镜像。
其余九个组件使用成功原生 CI 任务的摘要恢复清单。
发行叶清单逐一追溯至原生任务产物。
四个 Helm 包没有覆盖或替换。

## 集群与持久数据

入口为 <http://10.169.1.239:30820>。
命名空间为 `artifactory`。
三个 Helm 发布均为 `0.2.6`、`deployed`。

| Helm 发布 | 原 revision | 最终 revision |
| --- | --- | --- |
| artifactory | 45 | 47 |
| planning-operator | 7 | 8 |
| image-operator | 4 | 5 |

主发布 revision 46 遇到历史 SSA 字段冲突。
冲突字段为网关 Service 的 Chart 标签，以及 Control/Web 的配置校验注解。
恢复补丁校验 UID、资源版本和旧字段值。
补丁仅写入已通过服务器预演的三个字段。
revision 47 完成收敛，保留 revision 46 的失败历史。

业务、遥测、规划三个数据库均有发布前备份。
备份通过摘要、目录和完整解码校验。
九个 Deployment、15 个服务 Pod 全部就绪。
最终采样的服务 Pod 重启次数均为零。
六个一方 Deployment 的实际摘要匹配发布回执。
两个执行池使用本次固定镜像。
原 Secret、PVC、CRD、发行信任和安装身份通过核验。
原业务行、迁移、设备身份和项目行通过前后校验。

## 真实页面与构建

浏览器从登录进入工作空间大厅和项目目录。
1440px 和 375px 下检查项目、概览及设备页。
六个页面没有横向溢出和页面脚本错误。
顶栏玻璃读回为 `blur(10px) saturate(2.1)`。

真实 AI 规划及 BuildKit 构建成功。
构建 ID 为 `c14b4c0b-799e-4430-9439-f1367988de62`。
源码固定为 `c54160055c0ffcb6ef6191679231956083390252`。
候选摘要为 `sha256:262db523f9e20392f799f9f4b94335ee42702c37410188773285b6434beb1530`。
测试版本保持 `draft`，没有正式发布应用。
仅新增测试项目、版本、构建和 Job 各一条。
临时上传 Pod 与四项 Git 夹具资源均已按 UID 清理。

## 原生设备与 CI 边界

服务接口读回产品版本 `0.2.6`。
三台原生设备继续运行 `0.2.5`。
设备均在线，遥测新鲜度和启动身份核验通过。
普通产品发行不包含六平台签名 OTA 资产。
因此原生发行接口返回 `release_status: missing`。
本轮没有发起设备 OTA。

[源码 CI](https://github.com/zhubby/artifactory/actions/runs/45996173428)的前端、两个 Kubernetes 验收、Linux 端到端及其他五个平台原生任务通过。
Linux ARM 原生满盘恢复测试首次运行超时。
同一任务的协议、生命周期及持久恢复检查通过。
失败任务已单独重跑，本文记录时仍在运行。
镜像流水线的原始状态未因手工恢复改为成功。

私有机器回执、备份和截图保存在 `/tmp/artifactory-k8s-prism-20261127/`。
此部署记录的提交不改变 `v0.2.6` 的源码或运行镜像。
