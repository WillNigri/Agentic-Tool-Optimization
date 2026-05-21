import { describe, it, expect } from "vitest";
import {
  manualUpdateCommand,
  debReleaseUrl,
} from "@/components/UpdateBanner";

describe("UpdateBanner — manualUpdateCommand", () => {
  it("returns wget + apt install for .deb installs", () => {
    const cmd = manualUpdateCommand(
      "deb",
      "2.7.7",
      "https://example.com/ATO_2.7.7_amd64.deb",
    );
    expect(cmd).toBe(
      "wget https://example.com/ATO_2.7.7_amd64.deb -O /tmp/ato.deb && sudo apt install -y /tmp/ato.deb",
    );
  });

  it("returns `snap refresh` for snap installs", () => {
    expect(manualUpdateCommand("snap", "2.7.7", "ignored")).toBe(
      "sudo snap refresh ato",
    );
  });

  it("returns null for install methods with no manual path", () => {
    const url = "https://example.com/x.deb";
    expect(manualUpdateCommand("appimage", "2.7.7", url)).toBeNull();
    expect(manualUpdateCommand("unknown", "2.7.7", url)).toBeNull();
    expect(manualUpdateCommand("nonlinux", "2.7.7", url)).toBeNull();
  });

  it("rejects versions containing shell metacharacters", () => {
    const url = "https://example.com/x.deb";
    expect(manualUpdateCommand("deb", "2.7.7\n && rm -rf /", url)).toBeNull();
    expect(manualUpdateCommand("deb", "2.7.7`evil`", url)).toBeNull();
    expect(manualUpdateCommand("deb", "$(curl evil)", url)).toBeNull();
    expect(manualUpdateCommand("deb", "", url)).toBeNull();
  });

  it("rejects release URLs with shell metacharacters or bad schemes", () => {
    expect(
      manualUpdateCommand("deb", "2.7.7", "https://example.com/x.deb; rm -rf /"),
    ).toBeNull();
    expect(manualUpdateCommand("deb", "2.7.7", "file:///etc/passwd")).toBeNull();
    expect(
      manualUpdateCommand("deb", "2.7.7", "javascript:alert(1)"),
    ).toBeNull();
  });
});

describe("UpdateBanner — debReleaseUrl", () => {
  it("builds the GitHub release asset URL from a version", () => {
    expect(debReleaseUrl("2.7.7")).toBe(
      "https://github.com/WillNigri/Agentic-Tool-Optimization/releases/download/v2.7.7/ATO_2.7.7_amd64.deb",
    );
  });
});
