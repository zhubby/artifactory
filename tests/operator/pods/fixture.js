/**
 * [INPUT]: 依赖 Pod 身份、测试任务 JSON 与共享 socket 卷。
 * [OUTPUT]: 提供同名进程的成功、崩溃、延时与退出证据。
 * [POS]: 只验证 Operator 生命周期；不替代 Git、规划或 BuildKit 业务验收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
const fs = require('node:fs');
const path = require('node:path');
const command = path.basename(process.argv[1]);
const mode = process.argv[2];
const pod = process.env.POD_NAME || '';
const identity = JSON.parse(process.env.ARTIFACTORY_SOURCE_IDENTITY || '{}');
let seconds = 1;
let exitCode = 0;

if (command === 'artifactory-source-preparer') {
  if (mode !== 'checkout' && pod.includes('-a1-')) exitCode = 17;
  if (identity.run_id?.endsWith('000000000003')) exitCode = 31;
  if (identity.task_id?.endsWith('000000000004')) seconds = 600;
} else if (command === 'pod-main.js') {
  seconds = process.env.ARTIFACTORY_RUN_ID.endsWith('000000000001') ? 1 : 600;
  exitCode = seconds === 1 ? 23 : 0;
} else if (command === 'artifactory-image-buildkit') {
  const timer = setInterval(() => {
    if (fs.existsSync('/run/buildkit/fixture.json')) {
      clearInterval(timer);
      const task = JSON.parse(fs.readFileSync('/run/buildkit/fixture.json', 'utf8'));
      finish(task.sleepSeconds + 0.5, task.buildkitExitCode || 0);
    }
  }, 50);
  return;
} else {
  const task = JSON.parse(fs.readFileSync(process.env.ARTIFACTORY_IMAGE_TASK, 'utf8'));
  seconds = task.sleepSeconds;
  exitCode = task.exitCode || 0;
  if (command === 'artifactory-image-runner') {
    fs.writeFileSync('/run/buildkit/fixture.json', JSON.stringify(task));
  }
}
finish(seconds, exitCode);

function finish(delay, code) {
  console.log(JSON.stringify({ command, pod, uid: process.env.POD_UID, exitCode: code }));
  setTimeout(() => process.exit(code), delay * 1000);
}
