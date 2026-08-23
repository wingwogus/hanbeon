export const FIRMWARE_EVENT = "arduino://firmware";

export type ArduinoCandidate = {
  deviceId: string;
  displayName: string;
  port: string;
  vid: number;
  pid: number;
};

export type FirmwareState =
  | { state: "idle" }
  | { state: "searching" }
  | { state: "boardFound"; candidates: ArduinoCandidate[] }
  | { state: "probing"; deviceId: string }
  | { state: "alreadyInstalled"; deviceId: string }
  | {
      state: "confirmationRequired";
      deviceId: string;
      reason: "noResponse" | "differentFirmware";
    }
  | { state: "preparing"; deviceId: string }
  | { state: "uploading"; deviceId: string; progress?: number }
  | { state: "verifying"; deviceId: string }
  | { state: "complete"; deviceId: string }
  | { state: "cancelled" }
  | {
      state: "error";
      code: string;
      retryable: boolean;
    };

export const INITIAL_FIRMWARE_STATE: FirmwareState = { state: "idle" };

export function firmwareStatusText(state: FirmwareState): string {
  switch (state.state) {
    case "idle":
      return "";
    case "searching":
      return "Arduino Uno를 찾는 중입니다";
    case "boardFound":
      return "Arduino Uno를 찾았습니다";
    case "probing":
      return "기존 펌웨어 확인 중";
    case "alreadyInstalled":
      return "전용 펌웨어가 이미 설치되어 있습니다";
    case "confirmationRequired":
      return state.reason === "differentFirmware"
        ? "다른 스케치가 설치되어 있습니다"
        : "전용 펌웨어가 필요합니다";
    case "preparing":
      return "Arduino 준비 중";
    case "uploading":
      return "펌웨어 전송 중";
    case "verifying":
      return "설치 확인 중";
    case "complete":
      return "펌웨어 설치가 완료되었습니다";
    case "cancelled":
      return "펌웨어 설치를 취소했습니다";
    case "error":
      return "펌웨어 설치 중 문제가 발생했습니다";
  }
}
