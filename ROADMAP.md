# ATO Roadmap

## Code Signing & Notarization (macOS)

**Priority**: High — eliminates "app is damaged" warning for all macOS users

### Steps to get Apple Developer certificates:

1. **Enroll in Apple Developer Program**
   - Go to https://developer.apple.com/programs/
   - Sign in with your Apple ID
   - Pay $99/year (individual) or $299/year (organization)
   - Wait for approval (usually 24-48 hours)

2. **Create a Developer ID Application certificate**
   - Open Keychain Access on your Mac
   - Go to Keychain Access > Certificate Assistant > Request a Certificate from a Certificate Authority
   - Enter your email, select "Saved to disk", click Continue
   - Go to https://developer.apple.com/account/resources/certificates/add
   - Select "Developer ID Application"
   - Upload the CSR file you created
   - Download the certificate and double-click to install in Keychain

3. **Export the certificate as .p12**
   - In Keychain Access, find the "Developer ID Application" certificate
   - Right-click > Export
   - Save as .p12 format
   - Set a strong password (you'll need this as `APPLE_CERTIFICATE_PASSWORD`)

4. **Create an app-specific password for notarization**
   - Go to https://appleid.apple.com/account/manage
   - Sign in > Security > App-Specific Passwords > Generate
   - Label it "ATO Notarization"
   - Save the generated password

5. **Get your Team ID**
   - Go to https://developer.apple.com/account/#/membership
   - Copy your Team ID

6. **Add secrets to GitHub repository**
   - Go to https://github.com/WillNigri/Agentic-Tool-Optimization/settings/secrets/actions
   - Add these secrets:
     - `APPLE_CERTIFICATE` — Base64-encoded .p12 file: `base64 -i certificate.p12 | pbcopy`
     - `APPLE_CERTIFICATE_PASSWORD` — The password you set when exporting .p12
     - `APPLE_ID` — Your Apple ID email
     - `APPLE_PASSWORD` — The app-specific password from step 4
     - `APPLE_TEAM_ID` — Your Team ID from step 5

7. **Update release.yml** — Add signing env vars to the tauri-action step:
   ```yaml
   - name: Build Tauri app
     uses: tauri-apps/tauri-action@v0.5
     env:
       GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
       APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
       APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
       APPLE_ID: ${{ secrets.APPLE_ID }}
       APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
       APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
   ```

### Cost
- $99/year for Apple Developer Program

### Result
- macOS users can open ATO without Terminal commands
- No "app is damaged" warnings
- Gatekeeper shows "Apple checked it for malicious software" instead

---

## Future Features

- [ ] Apple code signing & notarization (see above)
- [ ] Windows code signing (EV certificate ~$200-400/year)
- [ ] Auto-updater (Tauri built-in updater with GitHub releases)
- [ ] Real Tauri backend commands (replace mock data with actual file reading)
- [ ] MCP server integration (connect prompt bar to real Claude Code MCP)
- [ ] Plugin marketplace / discovery
- [ ] Skill template library
- [ ] Export/import skill packs
- [ ] Team sync (cloud backend)
