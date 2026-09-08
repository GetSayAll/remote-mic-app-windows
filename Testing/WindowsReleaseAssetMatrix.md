# Windows 发布资产矩阵

| 资产 | 必需检查 | 通过标准 |
| --- | --- | --- |
| NSIS 安装器 | 文件名、版本/Build、PE 签名状态 | Preview 明确未签名；Stable 满足 Authenticode 策略 |
| `.sig` | minisign 格式与对应安装器 | 验签通过且字节一一对应 |
| `latest.json` | HTTPS URL、版本、平台键、签名内容 | 可解析、签名字段为 `.sig` 内容 |
| `SHA256SUMS.txt` | 每个公开资产 | 摘要与本地 staging 逐字节一致 |
| `build-metadata.json` | source SHA、channel、distribution status | 来源完整且不含秘密 |

上传前打印单一资产清单并核对大小和摘要。远端已有资产或无法判断的响应必须停止，不删除、不覆盖、不猜测来源。
