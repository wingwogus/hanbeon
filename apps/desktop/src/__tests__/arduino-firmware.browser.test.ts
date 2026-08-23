import { describe, expect, test } from "bun:test";

import {
  FIRMWARE_EVENT,
  INITIAL_FIRMWARE_STATE,
  firmwareStatusText,
  type ArduinoCandidate,
  type FirmwareState,
} from "../lib/firmware";

describe("Arduino firmware lifecycle contract", () => {
  test("uses a dedicated Tauri event and idle initial state", () => {
    expect(FIRMWARE_EVENT).toBe("arduino://firmware");
    expect(INITIAL_FIRMWARE_STATE).toEqual({ state: "idle" });
  });

  test("keeps candidate identity separate from the transient port path", () => {
    const candidate: ArduinoCandidate = {
      deviceId: "candidate-1",
      displayName: "Arduino Uno",
      port: "/dev/cu.usbmodem1401",
      vid: 0x2341,
      pid: 0x0043,
    };

    expect(candidate.deviceId).toBe("candidate-1");
    expect(candidate.port).toBe("/dev/cu.usbmodem1401");
  });

  test("renders the three user-facing installation phases", () => {
    const phases: FirmwareState[] = [
      { state: "preparing", deviceId: "candidate-1" },
      { state: "uploading", deviceId: "candidate-1" },
      { state: "verifying", deviceId: "candidate-1" },
    ];

    expect(phases.map(firmwareStatusText)).toEqual([
      "Arduino 준비 중",
      "펌웨어 전송 중",
      "설치 확인 중",
    ]);
  });

  test("distinguishes no response from different firmware", () => {
    expect(
      firmwareStatusText({
        state: "confirmationRequired",
        deviceId: "candidate-1",
        reason: "noResponse",
      }),
    ).toBe("전용 펌웨어가 필요합니다");
    expect(
      firmwareStatusText({
        state: "confirmationRequired",
        deviceId: "candidate-1",
        reason: "differentFirmware",
      }),
    ).toBe("다른 스케치가 설치되어 있습니다");
  });
});
