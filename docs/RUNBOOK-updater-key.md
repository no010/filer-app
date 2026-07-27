# Runbook：Tauri updater 签名密钥的备份与恢复

> 目标读者：filer 维护者（当前 owner：Peter）。
> 本文件**不含任何密钥内容或密码**，只描述流程；密钥文件永远不进 git（见 `.gitignore`）。

## 这把密钥是什么、为什么丢不起

| 项 | 说明 |
|---|---|
| 私钥文件 | `src-tauri/filer-updater.key`（本地，已 gitignore） |
| 私钥密码 | `src-tauri/filer-updater.key.pw`（本地，已 gitignore） |
| 对应公钥 | `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` |
| CI 使用 | repo secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（release.yml 构建时签名 `.sig` + `latest.json`） |

**丢失后果**：私钥或密码任一丢失 → 只能重新生成密钥对并更换 `pubkey` →
**所有按旧公钥安装的用户永远收不到自动更新**（应用内更新器验签失败），只能靠用户手动重装。
历史上已经发生过两次密钥重建（其中一次因密码丢失），不能有第三次。

## 备份决策（本项目采用的路线）

私钥 + 密码各存三处，任一处存活即可恢复：

1. **密码管理器**（主备份）：一条条目同时保存 `filer-updater.key` 的完整内容和密码。
2. **GitHub repo secrets**（已有）：`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 本身就是一份异地副本，可从 CI 配置侧恢复构建能力（但 secrets 无法读回明文，只能继续用于 CI 签名，不算完整备份）。
3. **离线介质**（灾备）：加密压缩包存 U 盘/网盘私密目录，内含两个文件。

> ⚠️ 每次执行密钥相关操作后，立刻核对三处备份是否同步。

## 恢复流程

### 场景 A：本地文件丢了，但备份还在
1. 从密码管理器（或离线介质）取回两个文件，放回 `src-tauri/`。
2. 验证：本地跑一次 `npm run tauri build`（或等下次 tag 发布），确认产物 `.sig` 能被现有版本应用验签更新。
3. **恢复完成的判定（postcondition）**：旧版本应用能检测到并成功安装新签名的更新，`pubkey` 未变更。

### 场景 B：所有备份都丢了（最坏情况）
1. `npm run tauri signer generate` 重新生成密钥对，密码当场写入密码管理器。
2. 更新 `tauri.conf.json` 的 `pubkey`、GitHub repo secrets、三处备份。
3. 发一个新版本；**接受损失**：旧安装用户不再收到自动更新，需在 README / Release 说明中提示手动升级一次。
4. **postcondition**：新装用户 + 手动升级过的用户恢复自动更新链路；三处备份齐备。

## 例行检查（每次发版顺手做）

- [ ] 本地两个密钥文件存在且未进 git（`git status` 不出现）
- [ ] 密码管理器条目存在
- [ ] repo secrets 未被误删（Actions 构建产物含 `.sig` 即为正常）
