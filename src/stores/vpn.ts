import type { UnlistenFn } from "@tauri-apps/api/event";
import { defineStore } from "pinia";
import { ref } from "vue";

import { normalizeAppError } from "../api/errors";
import { isDesktopRuntime } from "../api/settings";
import {
  connectProfile as connectProfileApi,
  disconnectProfile as disconnectProfileApi,
  listenForConnectionUpdates,
  listConnections,
  listRouteConflicts,
} from "../api/vpn";
import type { ConnectionAction, RouteConflict, VpnConnection, VpnRoute } from "../types/vpn";

export const useVpnStore = defineStore("vpn", () => {
  const connections = ref<Record<string, VpnConnection>>({});
  const pendingActions = ref<Record<string, ConnectionAction | undefined>>({});
  const profileErrors = ref<Record<string, string | undefined>>({});
  const routeConflicts = ref<RouteConflict[]>([]);
  const errorMessage = ref("");
  let unlistenPromise: Promise<UnlistenFn> | null = null;
  let conflictRefreshSequence = 0;

  function disconnectedConnection(profileId: string): VpnConnection {
    return {
      profileId,
      state: "disconnected",
      processId: null,
      managementPort: null,
      connectedAt: null,
      errorMessage: null,
      bytesReceived: 0,
      bytesSent: 0,
      tunnelAddress: null,
      remoteAddress: null,
      tunnelInterface: null,
      routes: [],
    };
  }

  function registerProfile(profileId: string): void {
    connections.value[profileId] ??= disconnectedConnection(profileId);
  }

  function forgetProfile(profileId: string): void {
    const { [profileId]: _connection, ...remainingConnections } = connections.value;
    const { [profileId]: _action, ...remainingActions } = pendingActions.value;
    const { [profileId]: _error, ...remainingErrors } = profileErrors.value;
    connections.value = remainingConnections;
    pendingActions.value = remainingActions;
    profileErrors.value = remainingErrors;
  }

  function applyConnection(connection: VpnConnection): void {
    const routesChanged =
      routeSignature(connections.value[connection.profileId]?.routes ?? []) !==
      routeSignature(connection.routes);
    connections.value[connection.profileId] = connection;
    if (routesChanged && isDesktopRuntime) {
      void refreshRouteConflicts().catch((error: unknown) => {
        errorMessage.value = normalizeAppError(error).message;
      });
    }
  }

  function routeSignature(routes: VpnRoute[]): string {
    return routes
      .map((route) => `${route.network}|${route.gateway ?? ""}|${route.source}`)
      .sort()
      .join(";");
  }

  async function refreshRouteConflicts(): Promise<void> {
    if (!isDesktopRuntime) {
      routeConflicts.value = [];
      return;
    }
    const sequence = ++conflictRefreshSequence;
    const conflicts = await listRouteConflicts();
    if (sequence === conflictRefreshSequence) routeConflicts.value = conflicts;
  }

  function connectionFor(profileId: string): VpnConnection {
    return connections.value[profileId] ?? disconnectedConnection(profileId);
  }

  async function initialize(profileIds: string[]): Promise<void> {
    profileIds.forEach(registerProfile);
    errorMessage.value = "";
    if (!isDesktopRuntime) {
      return;
    }

    if (!unlistenPromise) {
      unlistenPromise = listenForConnectionUpdates(applyConnection);
    }

    const listener = unlistenPromise;
    try {
      await listener;
      const [activeConnections] = await Promise.all([
        listConnections(),
        refreshRouteConflicts(),
      ]);
      activeConnections.forEach(applyConnection);
    } catch (error: unknown) {
      if (unlistenPromise === listener) unlistenPromise = null;
      errorMessage.value = normalizeAppError(error).message;
    }
  }

  function stopListening(): void {
    const pendingUnlisten = unlistenPromise;
    unlistenPromise = null;
    void pendingUnlisten?.then((unlisten) => unlisten()).catch(() => undefined);
  }

  async function connect(profileId: string): Promise<void> {
    await runAction(profileId, "connect", connectProfileApi);
  }

  async function disconnect(profileId: string): Promise<void> {
    await runAction(profileId, "disconnect", disconnectProfileApi);
  }

  async function runAction(
    profileId: string,
    action: ConnectionAction,
    operation: (profileId: string) => Promise<VpnConnection>,
  ): Promise<void> {
    if (pendingActions.value[profileId]) {
      return;
    }

    pendingActions.value[profileId] = action;
    profileErrors.value[profileId] = undefined;
    try {
      const result = await operation(profileId);
      const currentState = connectionFor(profileId).state;
      const resultIsStaleConnectingState =
        action === "connect" &&
        result.state === "connecting" &&
        currentState !== "disconnected" &&
        currentState !== "connecting";
      if (!resultIsStaleConnectingState) applyConnection(result);
      try {
        await refreshRouteConflicts();
      } catch (error: unknown) {
        errorMessage.value = normalizeAppError(error).message;
      }
    } catch (error: unknown) {
      profileErrors.value[profileId] = normalizeAppError(error).message;
    } finally {
      pendingActions.value[profileId] = undefined;
    }
  }

  return {
    connections,
    pendingActions,
    profileErrors,
    routeConflicts,
    errorMessage,
    connectionFor,
    registerProfile,
    forgetProfile,
    initialize,
    stopListening,
    connect,
    disconnect,
  };
});
