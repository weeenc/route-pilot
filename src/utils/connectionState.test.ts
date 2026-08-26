import { describe, expect, it } from "vitest";

import {
  canDisconnectState,
  isConnectedState,
  isConnectingState,
} from "./connectionState";

describe("connection state predicates", () => {
  it("keeps active-state behavior consistent across connection cards", () => {
    expect(isConnectedState("connected")).toBe(true);
    expect(isConnectedState("reconnecting")).toBe(true);
    expect(isConnectingState("connecting")).toBe(true);
    expect(isConnectingState("reconnecting")).toBe(true);
    expect(canDisconnectState("disconnecting")).toBe(true);
    expect(canDisconnectState("disconnected")).toBe(false);
    expect(canDisconnectState("error")).toBe(false);
  });
});
