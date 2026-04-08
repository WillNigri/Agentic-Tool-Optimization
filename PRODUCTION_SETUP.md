# ATO Production Setup Guide

This guide covers everything needed to deploy ATO for production use.

## Quick Start Checklist

- [ ] Generate Tauri signing keys
- [ ] Add signing keys to GitHub secrets
- [ ] Configure code signing (macOS/Windows)
- [ ] Set up notification providers
- [ ] Configure telemetry endpoint (optional)
- [ ] Create first release

---

## 1. Tauri Auto-Updater Setup

The auto-updater requires a signing key pair to verify updates.

### 1.1 Generate Signing Keys

```bash
# Install Tauri CLI if not already installed
cargo install tauri-cli

# Generate a new key pair (you'll be prompted for a password)
cargo tauri signer generate -w ~/.tauri/ato.key
```

This creates:
- **Private key**: `~/.tauri/ato.key` (KEEP SECRET!)
- **Public key**: Printed to console (add to `tauri.conf.json`)

### 1.2 Add Keys to GitHub Secrets

Go to your GitHub repository → Settings → Secrets and Variables → Actions

Add these secrets:
| Secret Name | Value |
|-------------|-------|
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of `~/.tauri/ato.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password you used during generation |

### 1.3 Update Public Key

Update `apps/desktop/src-tauri/tauri.conf.json`:

```json
{
  "plugins": {
    "updater": {
      "endpoints": [
        "https://github.com/YOUR_ORG/Agentic-Tool-Optimization/releases/latest/download/latest.json"
      ],
      "pubkey": "YOUR_PUBLIC_KEY_HERE"
    }
  }
}
```

---

## 2. Code Signing

### 2.1 macOS Code Signing & Notarization

**Requirements:**
- Apple Developer Program membership ($99/year)
- Developer ID Application certificate
- Developer ID Installer certificate (for DMG)

**Steps:**

1. Create certificates at [Apple Developer Portal](https://developer.apple.com/account/resources/certificates)

2. Download and install in Keychain

3. Add to GitHub Secrets:
```
APPLE_CERTIFICATE: Base64-encoded .p12 file
APPLE_CERTIFICATE_PASSWORD: Password for the .p12
APPLE_ID: Your Apple ID email
APPLE_PASSWORD: App-specific password (create at appleid.apple.com)
APPLE_TEAM_ID: Your team ID (found in developer portal)
```

4. Update `.github/workflows/release.yml`:
```yaml
- name: Build Tauri app
  uses: tauri-apps/tauri-action@v0
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
    APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
    APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
    APPLE_ID: ${{ secrets.APPLE_ID }}
    APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
    APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
```

### 2.2 Windows Code Signing

**Requirements:**
- EV Code Signing Certificate (from DigiCert, Sectigo, etc.)
- USB token or cloud signing service

**Steps:**

1. Purchase an EV code signing certificate

2. For cloud signing (Azure SignTool):
```
AZURE_KEY_VAULT_URI: https://your-vault.vault.azure.net
AZURE_CLIENT_ID: Your Azure AD application ID
AZURE_CLIENT_SECRET: Your application secret
AZURE_CERT_NAME: Certificate name in Key Vault
AZURE_TENANT_ID: Your Azure tenant ID
```

3. Update the release workflow to include Windows signing step

---

## 3. Notification Providers

### 3.1 Email (SMTP)

For Gmail:
1. Go to [Google Account Security](https://myaccount.google.com/security)
2. Enable 2-Step Verification
3. Generate an App Password (Security → App Passwords)

Configuration in ATO:
- **Host**: `smtp.gmail.com`
- **Port**: `587`
- **Username**: Your Gmail address
- **Password**: The App Password (not your Gmail password!)
- **From**: Your Gmail address
- **Use TLS**: Enabled

For other providers:
| Provider | Host | Port |
|----------|------|------|
| Gmail | smtp.gmail.com | 587 |
| Outlook | smtp.office365.com | 587 |
| SendGrid | smtp.sendgrid.net | 587 |
| Mailgun | smtp.mailgun.org | 587 |

### 3.2 Slack

1. Go to [Slack API](https://api.slack.com/apps)
2. Click "Create New App" → "From scratch"
3. Name it "ATO Notifications", select your workspace
4. Go to "Incoming Webhooks" → Enable
5. Click "Add New Webhook to Workspace"
6. Select a channel and authorize
7. Copy the Webhook URL

Configuration in ATO:
- **Webhook URL**: `https://hooks.slack.com/services/...`
- **Channel**: (optional) Override channel like `#alerts`

### 3.3 Discord

1. Go to your Discord server → Server Settings
2. Integrations → Webhooks → New Webhook
3. Configure name, avatar, and channel
4. Copy Webhook URL

Configuration in ATO:
- **Webhook URL**: `https://discord.com/api/webhooks/...`

### 3.4 Telegram

1. Message [@BotFather](https://t.me/botfather) on Telegram
2. Send `/newbot` and follow prompts
3. Copy the Bot Token
4. Add your bot to a group/channel
5. Get Chat ID:
   - For groups: Add bot, send message, visit `https://api.telegram.org/bot<TOKEN>/getUpdates`
   - For channels: Use the channel username with `-100` prefix

Configuration in ATO:
- **Bot Token**: `123456789:ABCdefGHI...`
- **Chat ID**: `-1001234567890`

---

## 4. Telemetry Setup (Optional)

ATO can send anonymous usage analytics to help improve the product.

### 4.1 Self-Hosted

Set up a simple endpoint to receive events:

```javascript
// Example Express.js endpoint
app.post('/telemetry', (req, res) => {
  const event = req.body;
  // Store in your analytics database
  console.log('Event:', event);
  res.status(200).send('OK');
});
```

### 4.2 PostHog (Recommended)

1. Create account at [PostHog](https://posthog.com)
2. Get your project API key
3. Set endpoint in ATO: `https://app.posthog.com/capture`

### 4.3 Custom Analytics

ATO sends events in this format:
```json
{
  "eventType": "app_launched",
  "properties": {
    "platform": "darwin",
    "userAgent": "...",
    "screenWidth": 2560
  },
  "timestamp": "2024-01-15T10:30:00Z",
  "sessionId": "uuid",
  "deviceId": "uuid"
}
```

Event types include:
- `app_launched` - App started
- `signup_completed` - User signed up
- `skill_created` - New skill created
- `automation_executed` - Workflow ran
- `feature_used` - Feature interaction
- `error_occurred` - Error happened

---

## 5. Creating a Release

### 5.1 Update Version

1. Update version in `apps/desktop/src-tauri/tauri.conf.json`
2. Update ROADMAP.md with release notes
3. Commit changes

### 5.2 Create Tag and Push

```bash
# Create and push a version tag
git tag v1.0.0
git push origin v1.0.0
```

The GitHub Actions workflow will automatically:
- Build for macOS (Intel + Apple Silicon)
- Build for Windows
- Build for Linux
- Create a GitHub Release
- Generate `latest.json` for auto-updater

### 5.3 Monitor Release

1. Go to GitHub Actions → Watch the build
2. Check the Release page for artifacts
3. Download and test on each platform

---

## 6. Troubleshooting

### Auto-updater not working

1. **Check endpoint URL**: Ensure `latest.json` is accessible at the endpoint
2. **Verify public key**: Must match the private key used for signing
3. **Check signatures**: Build must be signed with correct key

### Code signing failures

1. **macOS**: Ensure certificates are not expired
2. **Windows**: Check USB token is connected (for hardware tokens)
3. **Both**: Verify all environment variables are set correctly

### Notification failures

1. **Email**: Check SMTP credentials and port
2. **Slack/Discord**: Verify webhook URL is active
3. **Telegram**: Ensure bot has permission to post in chat

---

## 7. Security Checklist

- [ ] Signing keys are stored securely (not in repo)
- [ ] GitHub secrets are configured
- [ ] App passwords used instead of main passwords
- [ ] No hardcoded secrets in code
- [ ] CSP configured in tauri.conf.json
- [ ] Telemetry is opt-in
- [ ] Sensitive data not logged

---

## 8. Support

- **Issues**: https://github.com/WillNigri/Agentic-Tool-Optimization/issues
- **Discussions**: https://github.com/WillNigri/Agentic-Tool-Optimization/discussions
