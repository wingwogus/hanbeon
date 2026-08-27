package kr.devfive.hanbeon

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class BleSetupPermissionTest {
    @Test
    fun android31RequiresScanAndConnectWithoutAdvertise() {
        val permissions = BleSetupPolicy.runtimePermissions(apiLevel = 31)

        assertEquals(
            listOf(
                BleSetupPolicy.PERMISSION_SCAN,
                BleSetupPolicy.PERMISSION_CONNECT,
            ),
            permissions,
        )
        assertFalse(permissions.contains(BleSetupPolicy.PERMISSION_ADVERTISE))
        assertFalse(permissions.contains(BleSetupPolicy.PERMISSION_FINE_LOCATION))
    }

    @Test
    fun legacyApisRequestLocationNotScanConnectOrAdvertise() {
        val permissions = BleSetupPolicy.runtimePermissions(apiLevel = 30)

        assertEquals(listOf(BleSetupPolicy.PERMISSION_FINE_LOCATION), permissions)
        assertFalse(permissions.contains(BleSetupPolicy.PERMISSION_SCAN))
        assertFalse(permissions.contains(BleSetupPolicy.PERMISSION_CONNECT))
        assertFalse(permissions.contains(BleSetupPolicy.PERMISSION_ADVERTISE))
    }

    @Test
    fun activityLifecycleNeverAutoRequestsAfterDenialOrAtAll() {
        assertFalse(BleSetupPolicy.shouldRequestFromLifecycle(deniedOnce = false))
        assertFalse(BleSetupPolicy.shouldRequestFromLifecycle(deniedOnce = true))
        assertTrue(BleSetupPolicy.shouldRequestFromUserAction(deniedOnce = true))
    }

    @Test
    fun manifestDeclaresScanConnectNeverForLocationAndLegacyMaxSdkWithoutAdvertise() {
        val manifest = manifestFile().readText()

        assertTrue(manifest.contains("android.permission.BLUETOOTH_SCAN"))
        assertTrue(manifest.contains("android.permission.BLUETOOTH_CONNECT"))
        assertTrue(manifest.contains("neverForLocation"))
        assertTrue(manifest.contains("android.permission.BLUETOOTH\""))
        assertTrue(manifest.contains("maxSdkVersion=\"30\""))
        assertFalse(manifest.contains("BLUETOOTH_ADVERTISE"))
        assertFalse(manifest.contains("connectedDevice"))
    }
}

class BleSetupStateTest {
    @Test
    fun android31DenialIsActionableAndLeavesUsbUsable() {
        val snapshot =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = false,
                deniedOnce = true,
                adapterOn = true,
                selectionLabel = null,
                scanning = false,
                candidates = emptyList(),
            )

        assertEquals(BleSetupPolicy.CODE_PERMISSION_DENIED, snapshot.code)
        assertTrue(snapshot.usbUsable)
        assertFalse(snapshot.readyToConnect)
        assertTrue(snapshot.canRequestPermission)
        assertNull(snapshot.label)
        assertFalse(snapshot.toJson().contains("GATT", ignoreCase = true))
    }

    @Test
    fun bluetoothOffIsDistinctFromMissingSelection() {
        val snapshot =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = true,
                deniedOnce = false,
                adapterOn = false,
                selectionLabel = null,
                scanning = false,
                candidates = emptyList(),
            )

        assertEquals(BleSetupPolicy.CODE_BLUETOOTH_OFF, snapshot.code)
        assertFalse(snapshot.readyToConnect)
        assertTrue(snapshot.usbUsable)
    }

    @Test
    fun absentSelectionCannotConnect() {
        val snapshot =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = true,
                deniedOnce = false,
                adapterOn = true,
                selectionLabel = null,
                scanning = false,
                candidates = emptyList(),
            )

        assertEquals(BleSetupPolicy.CODE_NO_SELECTION, snapshot.code)
        assertFalse(snapshot.readyToConnect)
        assertTrue(snapshot.usbUsable)
    }

    @Test
    fun explicitSelectionIsTheOnlyConnectReadyState() {
        val snapshot =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = true,
                deniedOnce = false,
                adapterOn = true,
                selectionLabel = "HanBeon XIAO",
                scanning = false,
                candidates = emptyList(),
            )

        assertEquals(BleSetupPolicy.CODE_SELECTED, snapshot.code)
        assertTrue(snapshot.readyToConnect)
        assertEquals("HanBeon XIAO", snapshot.label)
    }

    @Test
    fun scanningWithoutSelectionIsNotConnectReady() {
        val snapshot =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = true,
                deniedOnce = false,
                adapterOn = true,
                selectionLabel = null,
                scanning = true,
                candidates = emptyList(),
            )

        assertEquals("scanning", snapshot.code)
        assertFalse(snapshot.readyToConnect)
        assertTrue(snapshot.usbUsable)
        assertTrue(snapshot.scanning)
        assertFalse(snapshot.toJson().contains("GATT", ignoreCase = true))
    }

    @Test
    fun permissionRevocationAfterSavedSelectionStaysActionableAndUsbUsable() {
        val snapshot =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = false,
                deniedOnce = true,
                adapterOn = true,
                selectionLabel = "HanBeon XIAO",
                scanning = false,
                candidates = emptyList(),
            )

        assertEquals(BleSetupPolicy.CODE_PERMISSION_DENIED, snapshot.code)
        assertFalse(snapshot.readyToConnect)
        assertTrue(snapshot.usbUsable)
        assertTrue(snapshot.canRequestPermission)
        assertFalse(snapshot.toJson().contains("AA:BB"))
    }
}

class BleSetupTrustTest {
    @Test
    fun untrustedExternalMetadataIsRejected() {
        assertFalse(
            BleSetupPolicy.isTrustedCandidate(
                name = "AA:BB:CC:DD:EE:FF",
                advertisedServiceUuids = listOf(BleSetupPolicy.NUS_SERVICE_UUID),
            ),
        )
        assertFalse(
            BleSetupPolicy.isTrustedCandidate(
                name = "Pixel Buds",
                advertisedServiceUuids = listOf(BleSetupPolicy.NUS_SERVICE_UUID),
            ),
        )
        assertFalse(
            BleSetupPolicy.isTrustedCandidate(
                name = "GATT error 133",
                advertisedServiceUuids = emptyList(),
            ),
        )
        assertFalse(
            BleSetupPolicy.isTrustedCandidate(
                name = null,
                advertisedServiceUuids = listOf(BleSetupPolicy.NUS_SERVICE_UUID),
            ),
        )
        assertTrue(
            BleSetupPolicy.isTrustedCandidate(
                name = "HanBeon XIAO",
                advertisedServiceUuids = emptyList(),
            ),
        )
    }

    @Test
    fun publicLabelNeverExposesMacOrGattErrors() {
        assertEquals("HanBeon XIAO", BleSetupPolicy.publicLabel("HanBeon XIAO"))
        assertEquals(
            BleSetupPolicy.SAFE_FALLBACK_LABEL,
            BleSetupPolicy.publicLabel("AA:BB:CC:DD:EE:FF"),
        )
        assertEquals(
            BleSetupPolicy.SAFE_FALLBACK_LABEL,
            BleSetupPolicy.publicLabel("status=133 GATT_ERROR"),
        )
        assertTrue(BleSetupPolicy.containsForbiddenIdentity("00:11:22:33:44:55"))
    }

    @Test
    fun candidateViewsDropRawAddresses() {
        val candidate =
            BleSetupPolicy.candidateView(
                token = "ble-1",
                name = "HanBeon XIAO",
                address = "AA:BB:CC:DD:EE:FF",
            )

        assertEquals("ble-1", candidate.token)
        assertEquals("HanBeon XIAO", candidate.label)
        assertFalse(candidate.toJson().contains("AA:BB:CC:DD:EE:FF"))
    }
}

class TrustedBleStoreTest {
    @Test
    fun selectRevokeAndReloadPersistExactlyOneSafeIdentity() {
        val file = File.createTempFile("trusted-ble", ".json")
        file.deleteOnExit()
        val store = TrustedBleStore(file)

        store.save(TrustedBleRecord("AA:BB:CC:DD:EE:FF", "Nearby speaker"))
        store.save(TrustedBleRecord("11:22:33:44:55:66", "HanBeon XIAO"))

        val loaded = TrustedBleStore(file).load()
        assertEquals("11:22:33:44:55:66", loaded?.identity)
        assertEquals("HanBeon XIAO", loaded?.label)
        assertEquals("HanBeon XIAO", store.publicSelectionLabel())

        val selected =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = true,
                deniedOnce = false,
                adapterOn = true,
                selectionLabel = store.publicSelectionLabel(),
                scanning = false,
                candidates = emptyList(),
            )
        assertEquals(BleSetupPolicy.CODE_SELECTED, selected.code)
        assertFalse(selected.toJson().contains("11:22:33:44:55:66"))
        assertFalse(selected.toJson().contains("identity"))
        assertFalse(selected.toJson().contains("AA:BB:CC:DD:EE:FF"))

        store.revoke()
        assertNull(TrustedBleStore(file).load())
        assertNull(store.publicSelectionLabel())
        val revoked =
            BleSetupPolicy.snapshot(
                hasBleHardware = true,
                permissionGranted = true,
                deniedOnce = false,
                adapterOn = true,
                selectionLabel = store.publicSelectionLabel(),
                scanning = false,
                candidates = emptyList(),
            )
        assertEquals(BleSetupPolicy.CODE_NO_SELECTION, revoked.code)
        assertFalse(revoked.readyToConnect)
        assertTrue(revoked.usbUsable)
        assertNotEquals(BleSetupPolicy.CODE_SELECTED, revoked.code)
    }

    @Test
    fun staleSavedMacLabelIsNotExposedInPublicState() {
        val file = File.createTempFile("trusted-ble-stale", ".json")
        file.deleteOnExit()
        file.writeText(
            """{"identity":"AA:BB:CC:DD:EE:FF","label":"AA:BB:CC:DD:EE:FF"}""",
        )

        val store = TrustedBleStore(file)
        assertEquals(BleSetupPolicy.SAFE_FALLBACK_LABEL, store.publicSelectionLabel())
        assertFalse(store.publicSelectionLabel()!!.contains(":"))
        assertTrue(store.load()?.identity == "AA:BB:CC:DD:EE:FF")
    }
}

class BleSetupTimeoutGenerationTest {
    @Test
    fun olderTimeoutCannotStopNewerScan() {
        val generations = BleScanGenerations()
        val first = generations.begin()
        generations.stop()
        val second = generations.begin()

        assertFalse(generations.claimTimeout(first))
        assertTrue(generations.claimTimeout(second))
        assertFalse(generations.claimTimeout(second))
    }
}

class BleSetupControllerTest {
    @Test
    fun selectRevokeAndPermissionLossKeepUsbAndHideIdentity() {
        val file = File.createTempFile("trusted-ble-ctrl", ".json")
        file.deleteOnExit()
        var granted = true
        var adapter = true
        val controller =
            BleSetupController(
                store = TrustedBleStore(file),
                apiLevel = 31,
                hasBleHardware = true,
                adapterOn = { adapter },
                permissionGranted = { granted },
                scan = { onResult ->
                    onResult(listOf(BleCandidateView("ble-1", "HanBeon XIAO")))
                },
            )

        controller.beginScan()
        val selected = controller.select("ble-1", "11:22:33:44:55:66", "HanBeon XIAO")
        assertEquals(BleSetupPolicy.CODE_SELECTED, selected.code)
        assertTrue(selected.readyToConnect)
        assertTrue(selected.usbUsable)
        assertFalse(selected.toJson().contains("11:22:33:44:55:66"))
        assertFalse(selected.toJson().contains("GATT", ignoreCase = true))

        granted = false
        val denied = controller.snapshot()
        assertEquals(BleSetupPolicy.CODE_PERMISSION_DENIED, denied.code)
        assertFalse(denied.readyToConnect)
        assertTrue(denied.usbUsable)
        assertFalse(BleSetupPolicy.shouldRequestFromLifecycle(deniedOnce = true))
        assertTrue(controller.requestPermissionIfAllowed())

        granted = true
        adapter = false
        val off = controller.snapshot()
        assertEquals(BleSetupPolicy.CODE_BLUETOOTH_OFF, off.code)
        assertFalse(off.readyToConnect)
        assertTrue(off.usbUsable)

        adapter = true
        val restored = controller.snapshot()
        assertEquals(BleSetupPolicy.CODE_SELECTED, restored.code)
        assertTrue(restored.readyToConnect)

        val revoked = controller.revoke()
        assertEquals(BleSetupPolicy.CODE_NO_SELECTION, revoked.code)
        assertFalse(revoked.readyToConnect)
        assertTrue(revoked.usbUsable)
        assertNull(TrustedBleStore(file).load())
    }
}

private fun manifestFile(): File {
    val roots =
        listOf(
            File("src/main/AndroidManifest.xml"),
            File("app/src/main/AndroidManifest.xml"),
            File("apps/desktop/src-tauri/gen/android/app/src/main/AndroidManifest.xml"),
        )
    return roots.firstOrNull { it.isFile }
        ?: error("AndroidManifest.xml was not found from the test working directory")
}
