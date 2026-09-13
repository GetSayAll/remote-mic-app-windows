#include "driver.h"
#include "remap.h"

#ifdef ALLOC_PRAGMA
#pragma alloc_text(INIT, DriverEntry)
#pragma alloc_text(PAGE, SayAllThreeButtonEvtDeviceAdd)
#endif

NTSTATUS
DriverEntry(
    _In_ PDRIVER_OBJECT DriverObject,
    _In_ PUNICODE_STRING RegistryPath
    )
{
    WDF_DRIVER_CONFIG config;

    WDF_DRIVER_CONFIG_INIT(&config, SayAllThreeButtonEvtDeviceAdd);

    return WdfDriverCreate(
        DriverObject,
        RegistryPath,
        WDF_NO_OBJECT_ATTRIBUTES,
        &config,
        WDF_NO_HANDLE);
}

NTSTATUS
SayAllThreeButtonEvtDeviceAdd(
    _In_ WDFDRIVER Driver,
    _Inout_ PWDFDEVICE_INIT DeviceInit
    )
{
    WDFDEVICE device;
    WDF_IO_QUEUE_CONFIG queueConfig;
    NTSTATUS status;

    UNREFERENCED_PARAMETER(Driver);
    PAGED_CODE();

    // Device-specific lower filter below kbdhid.sys.
    WdfFdoInitSetFilter(DeviceInit);

    status = WdfDeviceCreate(
        &DeviceInit,
        WDF_NO_OBJECT_ATTRIBUTES,
        &device);
    if (!NT_SUCCESS(status)) {
        return status;
    }

    // kbdhid continuously sends IRP_MJ_READ requests for raw HID reports.
    // Filter drivers automatically pass request types without a registered
    // callback to the next lower driver.
    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(
        &queueConfig,
        WdfIoQueueDispatchParallel);
    queueConfig.EvtIoRead = SayAllThreeButtonEvtIoRead;

    return WdfIoQueueCreate(
        device,
        &queueConfig,
        WDF_NO_OBJECT_ATTRIBUTES,
        WDF_NO_HANDLE);
}

VOID
SayAllThreeButtonEvtIoRead(
    _In_ WDFQUEUE Queue,
    _In_ WDFREQUEST Request,
    _In_ size_t Length
    )
{
    WDFDEVICE device;
    WDFIOTARGET target;
    NTSTATUS status;
    BOOLEAN sent;

    UNREFERENCED_PARAMETER(Length);

    device = WdfIoQueueGetDevice(Queue);
    target = WdfDeviceGetIoTarget(device);

    WdfRequestFormatRequestUsingCurrentType(Request);
    WdfRequestSetCompletionRoutine(
        Request,
        SayAllThreeButtonReadCompletion,
        WDF_NO_CONTEXT);

    sent = WdfRequestSend(Request, target, WDF_NO_SEND_OPTIONS);
    if (!sent) {
        status = WdfRequestGetStatus(Request);
        WdfRequestComplete(Request, status);
    }
}

VOID
SayAllThreeButtonReadCompletion(
    _In_ WDFREQUEST Request,
    _In_ WDFIOTARGET Target,
    _In_ PWDF_REQUEST_COMPLETION_PARAMS CompletionParams,
    _In_ WDFCONTEXT Context
    )
{
    NTSTATUS lowerStatus;
    ULONG_PTR information;
    PVOID buffer = NULL;
    SIZE_T bufferLength = 0;
    SIZE_T reportLength;

    UNREFERENCED_PARAMETER(Target);
    UNREFERENCED_PARAMETER(Context);

    lowerStatus = CompletionParams->IoStatus.Status;
    information = CompletionParams->IoStatus.Information;

    if (NT_SUCCESS(lowerStatus) && information >= 4) {
        // This is the original framework-delivered read request, so WDF keeps
        // its buffered/direct-I/O output buffer accessible until completion.
        NTSTATUS bufferStatus = WdfRequestRetrieveOutputBuffer(
            Request,
            4,
            &buffer,
            &bufferLength);

        if (NT_SUCCESS(bufferStatus) && bufferLength >= 4) {
            reportLength = (SIZE_T)information;
            if (reportLength > bufferLength) {
                reportLength = bufferLength;
            }

            if (reportLength >= 4) {
                NT_ANALYSIS_ASSUME(reportLength <= bufferLength);
                SayAllThreeButtonRemapReport((PUCHAR)buffer, reportLength);
            }
        }
    }

    WdfRequestCompleteWithInformation(
        Request,
        lowerStatus,
        information);
}
