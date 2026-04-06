/**
 * @file zoom_tool.cpp
 * @brief Professional Precision Zoom Tool with Scope Mode, Color Effects, and GDI+.
 * * Features:
 * - Follow-Mouse Zoom: The scope now follows the cursor for dynamic precision.
 * - Performance Optimized: Caches GDI+ images and avoids redundant disk I/O.
 * - Anti-Cheat Safe: Uses passive polling for keys (+/-) instead of hooks.
 * - Pixel-Perfect Centering: Fixed DPI scaling and monitor coordinate logic.
 * - Scope Border: Customizable stroke around the magnified area.
 * - Crosshair Offsets: Nudge PNG or procedural crosshairs for perfect zeroing.
 * - Circular Scope Mode: Toggle between square and round magnification.
 * - Negative Mode: High-contrast color inversion via Magnifier API.
 * - Opacity Control: Adjust transparency of the zoom window.
 * - Persistent Settings: Saves to Registry.
 * - Auto-Relaunch: Hard-clears DWM buffers on Apply.
 * * Compilation:
 * cl.exe /W4 /EHsc /O2 zoom_tool.cpp /link Magnification.lib User32.lib Gdi32.lib Shell32.lib Comctl32.lib Advapi32.lib Gdiplus.lib Ole32.lib Comdlg32.lib /SUBSYSTEM:WINDOWS
 */

#include <windows.h>
#include <magnification.h>
#include <shellapi.h>
#include <commctrl.h>
#include <gdiplus.h>
#include <thread>
#include <atomic>
#include <string>

#pragma comment(lib, "comctl32.lib")
#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "gdiplus.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "comdlg32.lib")

using namespace Gdiplus;

// --- Configuration & Constants ---
#define WM_TRAYICON (WM_USER + 1)
#define ID_TRAY_EXIT 1001
#define ID_TRAY_SETTINGS 1002
#define ID_APPLY_SETTINGS 1003
#define ID_RESET_SETTINGS 1004
#define ID_LOAD_PNG 1005
#define ID_UNLOAD_PNG 1006

const COLORREF TRANSPARENCY_KEY = RGB(255, 0, 255); 
const char* REG_KEY = "Software\\PrecisionZoomTool";

struct Config {
    float zoomFactor = 2.0f;
    int width = 350;
    int height = 350;
    int refreshRateMs = 10;
    bool showCrosshair = false;
    int crosshairSize = 32; 
    COLORREF crosshairColor = RGB(255, 0, 0);
    char pngPath[MAX_PATH] = {0};
    int triggerKey = VK_RBUTTON;    
    int modifierKey = VK_LSHIFT;
    
    bool circularScope = false;
    bool negativeMode = false;
    int opacity = 255;

    int offsetX = 0;
    int offsetY = 0;
    int borderThickness = 2;
    COLORREF borderColor = RGB(0, 0, 0); 

    void Reset() {
        zoomFactor = 2.0f;
        width = 350;
        height = 350;
        refreshRateMs = 10;
        showCrosshair = false;
        crosshairSize = 32;
        crosshairColor = RGB(255, 0, 0);
        memset(pngPath, 0, MAX_PATH);
        circularScope = false;
        negativeMode = false;
        opacity = 255;
        offsetX = 0;
        offsetY = 0;
        borderThickness = 2;
        borderColor = RGB(0, 0, 0);
    }

    void Load() {
        HKEY hKey;
        if (RegOpenKeyExA(HKEY_CURRENT_USER, REG_KEY, 0, KEY_READ, &hKey) == ERROR_SUCCESS) {
            DWORD dwType = REG_BINARY;
            DWORD dwSize = sizeof(Config);
            RegQueryValueExA(hKey, "Settings", NULL, &dwType, (LPBYTE)this, &dwSize);
            RegCloseKey(hKey);
        }
    }

    void Save() {
        HKEY hKey;
        if (RegCreateKeyExA(HKEY_CURRENT_USER, REG_KEY, 0, NULL, REG_OPTION_NON_VOLATILE, KEY_WRITE, NULL, &hKey, NULL) == ERROR_SUCCESS) {
            RegSetValueExA(hKey, "Settings", 0, REG_BINARY, (LPBYTE)this, sizeof(Config));
            RegCloseKey(hKey);
        }
    }
};

// --- Global State ---
Config g_Config;
std::atomic<bool> g_Running(true);
std::atomic<bool> g_MasterActive(true);
std::atomic<bool> g_IsZoomed(false);

// Resource Cache
Image* g_CachedImage = nullptr;

HWND g_hwndMag = NULL;
HWND g_hwndHost = NULL;
HWND g_hwndCrosshair = NULL; 
HWND g_hwndSettings = NULL;
NOTIFYICONDATA g_nid = {};

HWND g_hEditZoom, g_hEditSize, g_hEditRefresh, g_hCheckCross, g_hEditCrossSize, g_hStaticPng;
HWND g_hCheckCircle, g_hCheckNegative, g_hEditOpacity, g_hEditOffsetX, g_hEditOffsetY, g_hEditBorder;

/**
 * Loads the PNG into memory once to avoid disk I/O every frame.
 */
void LoadCrosshairImage() {
    if (g_CachedImage) {
        delete g_CachedImage;
        g_CachedImage = nullptr;
    }
    if (strlen(g_Config.pngPath) > 0) {
        wchar_t wPath[MAX_PATH];
        MultiByteToWideChar(CP_ACP, 0, g_Config.pngPath, -1, wPath, MAX_PATH);
        g_CachedImage = new Image(wPath);
        if (g_CachedImage->GetLastStatus() != Ok) {
            delete g_CachedImage;
            g_CachedImage = nullptr;
        }
    }
}

/**
 * Force a window to the foreground by attaching to the current foreground thread.
 */
void ForceForegroundWindow(HWND hwnd) {
    DWORD foregroundThreadID = GetWindowThreadProcessId(GetForegroundWindow(), NULL);
    DWORD currentThreadID = GetCurrentThreadId();
    if (foregroundThreadID != currentThreadID) {
        AttachThreadInput(foregroundThreadID, currentThreadID, TRUE);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        AttachThreadInput(foregroundThreadID, currentThreadID, FALSE);
    } else {
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
    }
}

/**
 * Excludes tool windows from magnification to prevent ghosting.
 */
void ExcludeToolWindows() {
    if (!g_hwndMag) return;
    HWND list[3];
    list[0] = g_hwndHost;
    list[1] = g_hwndCrosshair;
    list[2] = g_hwndSettings;
    MagSetWindowFilterList(g_hwndMag, MW_FILTERMODE_EXCLUDE, 3, list);

    typedef BOOL (WINAPI *pSetWindowDisplayAffinity)(HWND, DWORD);
    pSetWindowDisplayAffinity SetAffinity = (pSetWindowDisplayAffinity)GetProcAddress(GetModuleHandleA("user32.dll"), "SetWindowDisplayAffinity");
    if (SetAffinity) {
       // SetAffinity(g_hwndHost, 0x00000011); 
       // SetAffinity(g_hwndCrosshair, 0x00000011);
    }
}

void Relaunch() {
    char szFileName[MAX_PATH];
    GetModuleFileNameA(NULL, szFileName, MAX_PATH);
    AllowSetForegroundWindow(ASFW_ANY);
    ShellExecuteA(NULL, "open", szFileName, "--settings", NULL, SW_SHOW);
    exit(0);
}

/**
 * Magnifier update: Center the source capture on the host window's current position.
 */
void UpdateMagnifier() {
    if (!g_hwndMag || !g_hwndHost) return;

    RECT hostRect;
    GetWindowRect(g_hwndHost, &hostRect);

    int centerX = hostRect.left + (g_Config.width / 2);
    int centerY = hostRect.top + (g_Config.height / 2);

    int srcWidth = (int)((float)g_Config.width / g_Config.zoomFactor);
    int srcHeight = (int)((float)g_Config.height / g_Config.zoomFactor);

    RECT sourceRect;
    sourceRect.left = centerX - (srcWidth / 2);
    sourceRect.top = centerY - (srcHeight / 2);
    sourceRect.right = sourceRect.left + srcWidth;
    sourceRect.bottom = sourceRect.top + srcHeight;

    MAGTRANSFORM matrix;
    memset(&matrix, 0, sizeof(matrix));
    matrix.v[0][0] = g_Config.zoomFactor;
    matrix.v[1][1] = g_Config.zoomFactor;
    matrix.v[2][2] = 1.0f;
    
    MagSetWindowTransform(g_hwndMag, &matrix);
    MagSetWindowSource(g_hwndMag, sourceRect);

    if (g_Config.negativeMode) {
        MAGCOLOREFFECT effect = {
            -1.0f,  0.0f,  0.0f,  0.0f,  0.0f,
             0.0f, -1.0f,  0.0f,  0.0f,  0.0f,
             0.0f,  0.0f, -1.0f,  0.0f,  0.0f,
             0.0f,  0.0f,  0.0f,  1.0f,  0.0f,
             1.0f,  1.0f,  1.0f,  0.0f,  1.0f
        };
        MagSetColorEffect(g_hwndMag, &effect);
    } else {
        MagSetColorEffect(g_hwndMag, NULL);
    }
}

void InputPollingThread() {
    bool lastToggleState = false;
    while (g_Running) {
        bool mainKeyDown = (GetAsyncKeyState(g_Config.triggerKey) & 0x8000) != 0;
        bool modKeyDown = (GetAsyncKeyState(g_Config.modifierKey) & 0x8000) != 0;
        bool toggleComboActive = mainKeyDown && modKeyDown;
        
        if (toggleComboActive && !lastToggleState) {
            g_MasterActive = !g_MasterActive;
            if (!g_MasterActive && g_IsZoomed) {
                g_IsZoomed = false;
                ShowWindow(g_hwndHost, SW_HIDE);
                ShowWindow(g_hwndCrosshair, SW_HIDE);
            }
            if (g_MasterActive) MessageBeep(MB_OK); 
            else MessageBeep(MB_ICONHAND);
            strcpy_s(g_nid.szTip, g_MasterActive ? "Precision Zoom [ENABLED]" : "Precision Zoom [DISABLED]");
            Shell_NotifyIcon(NIM_MODIFY, &g_nid);
        }
        lastToggleState = toggleComboActive;

        if (g_MasterActive) {
            bool zoomShouldBeActive = mainKeyDown && !modKeyDown;
            
            if (zoomShouldBeActive) {
                POINT pt;
                GetCursorPos(&pt);
                // Center window on cursor
                int x = pt.x - (g_Config.width / 2);
                int y = pt.y - (g_Config.height / 2);
                SetWindowPos(g_hwndHost, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                SetWindowPos(g_hwndCrosshair, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);

                if (!g_IsZoomed) {
                    g_IsZoomed = true;
                    ExcludeToolWindows();
                    ShowWindow(g_hwndHost, SW_SHOWNA);
                    if (g_Config.showCrosshair) {
                        ShowWindow(g_hwndCrosshair, SW_SHOWNA);
                        InvalidateRect(g_hwndCrosshair, NULL, TRUE);
                        UpdateWindow(g_hwndCrosshair);
                    }
                }

                // Keyboard Polling for +/- Zoom
                if (GetAsyncKeyState(VK_OEM_PLUS) & 0x8000) {
                    g_Config.zoomFactor += 0.05f;
                    if (g_Config.zoomFactor > 15.0f) g_Config.zoomFactor = 15.0f;
                }
                if (GetAsyncKeyState(VK_OEM_MINUS) & 0x8000) {
                    g_Config.zoomFactor -= 0.05f;
                    if (g_Config.zoomFactor < 1.1f) g_Config.zoomFactor = 1.1f;
                }

                UpdateMagnifier();
            } else if (g_IsZoomed) {
                g_IsZoomed = false;
                ShowWindow(g_hwndHost, SW_HIDE);
                ShowWindow(g_hwndCrosshair, SW_HIDE);
            }
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(g_Config.refreshRateMs));
    }
}

LRESULT CALLBACK CrosshairProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    if (uMsg == WM_PAINT) {
        PAINTSTRUCT ps;
        HDC hdc = BeginPaint(hwnd, &ps);
        Graphics graphics(hdc);
        graphics.Clear(Color(255, 0, 255));

        int midX = (g_Config.width / 2) + g_Config.offsetX;
        int midY = (g_Config.height / 2) + g_Config.offsetY;
        int s = g_Config.crosshairSize;

        if (g_Config.borderThickness > 0) {
            graphics.SetSmoothingMode(SmoothingModeNone); 
            Pen borderPen(Color(255, GetRValue(g_Config.borderColor), GetGValue(g_Config.borderColor), GetBValue(g_Config.borderColor)), (REAL)g_Config.borderThickness);
            float offset = (float)g_Config.borderThickness / 2.0f;
            
            if (g_Config.circularScope) {
                graphics.DrawEllipse(&borderPen, offset, offset, (float)g_Config.width - g_Config.borderThickness, (float)g_Config.height - g_Config.borderThickness);
            } else {
                graphics.DrawRectangle(&borderPen, offset, offset, (float)g_Config.width - g_Config.borderThickness, (float)g_Config.height - g_Config.borderThickness);
            }
        }

        if (g_CachedImage) {
            ImageAttributes attr;
            attr.SetWrapMode(WrapModeTileFlipXY);
            graphics.SetPixelOffsetMode(PixelOffsetModeHalf);
            graphics.SetSmoothingMode(SmoothingModeAntiAlias);
            graphics.SetInterpolationMode(InterpolationModeHighQualityBicubic);
            graphics.DrawImage(g_CachedImage, Rect(midX - (s / 2), midY - (s / 2), s, s), 0, 0, (INT)g_CachedImage->GetWidth(), (INT)g_CachedImage->GetHeight(), UnitPixel, &attr);
        } else if (g_Config.showCrosshair) {
            HPEN hPen = CreatePen(PS_SOLID, 2, g_Config.crosshairColor);
            SelectObject(hdc, hPen);
            int arm = s / 2;
            MoveToEx(hdc, midX - arm, midY, NULL); LineTo(hdc, midX + arm, midY);
            MoveToEx(hdc, midX, midY - arm, NULL); LineTo(hdc, midX, midY + arm);
            DeleteObject(hPen);
        }

        EndPaint(hwnd, &ps);
        return 0;
    }
    return DefWindowProc(hwnd, uMsg, wParam, lParam);
}

LRESULT CALLBACK SettingsProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
        case WM_CREATE: {
            int y = 15; int labelW = 125; int editX = 145; int editW = 65;
            CreateWindow("STATIC", "Zoom Factor:", WS_VISIBLE | WS_CHILD, 10, y, labelW, 20, hwnd, NULL, NULL, NULL);
            g_hEditZoom = CreateWindow("EDIT", "", WS_VISIBLE | WS_CHILD | WS_BORDER, editX, y, editW, 20, hwnd, NULL, NULL, NULL); y += 30;
            CreateWindow("STATIC", "Box Size (px):", WS_VISIBLE | WS_CHILD, 10, y, labelW, 20, hwnd, NULL, NULL, NULL);
            g_hEditSize = CreateWindow("EDIT", "", WS_VISIBLE | WS_CHILD | WS_BORDER, editX, y, editW, 20, hwnd, NULL, NULL, NULL); y += 30;
            CreateWindow("STATIC", "Opacity (0-255):", WS_VISIBLE | WS_CHILD, 10, y, labelW, 20, hwnd, NULL, NULL, NULL);
            g_hEditOpacity = CreateWindow("EDIT", "", WS_VISIBLE | WS_CHILD | WS_BORDER, editX, y, editW, 20, hwnd, NULL, NULL, NULL); y += 30;
            CreateWindow("STATIC", "Border (px):", WS_VISIBLE | WS_CHILD, 10, y, labelW, 20, hwnd, NULL, NULL, NULL);
            g_hEditBorder = CreateWindow("EDIT", "", WS_VISIBLE | WS_CHILD | WS_BORDER, editX, y, editW, 20, hwnd, NULL, NULL, NULL); y += 35;
            g_hCheckCircle = CreateWindow("BUTTON", "Circular Scope Mode", WS_VISIBLE | WS_CHILD | BS_AUTOCHECKBOX, 10, y, 220, 20, hwnd, NULL, NULL, NULL); y += 25;
            g_hCheckNegative = CreateWindow("BUTTON", "Negative Mode (Contrast)", WS_VISIBLE | WS_CHILD | BS_AUTOCHECKBOX, 10, y, 220, 20, hwnd, NULL, NULL, NULL); y += 25;
            g_hCheckCross = CreateWindow("BUTTON", "Enable Crosshair", WS_VISIBLE | WS_CHILD | BS_AUTOCHECKBOX, 10, y, 220, 20, hwnd, NULL, NULL, NULL); y += 25;
            CreateWindow("STATIC", "X Offset:", WS_VISIBLE | WS_CHILD, 10, y, 70, 20, hwnd, NULL, NULL, NULL);
            g_hEditOffsetX = CreateWindow("EDIT", "", WS_VISIBLE | WS_CHILD | WS_BORDER, 80, y, 40, 20, hwnd, NULL, NULL, NULL); 
            CreateWindow("STATIC", "Y Offset:", WS_VISIBLE | WS_CHILD, 130, y, 70, 20, hwnd, NULL, NULL, NULL);
            g_hEditOffsetY = CreateWindow("EDIT", "", WS_VISIBLE | WS_CHILD | WS_BORDER, 200, y, 40, 20, hwnd, NULL, NULL, NULL); y += 30;
            CreateWindow("STATIC", "Scale:", WS_VISIBLE | WS_CHILD, 10, y, labelW, 20, hwnd, NULL, NULL, NULL);
            g_hEditCrossSize = CreateWindow("EDIT", "", WS_VISIBLE | WS_CHILD | WS_BORDER, editX, y, editW, 20, hwnd, NULL, NULL, NULL); y += 35;
            CreateWindow("BUTTON", "Load PNG", WS_VISIBLE | WS_CHILD, 10, y, 100, 25, hwnd, (HMENU)ID_LOAD_PNG, NULL, NULL);
            CreateWindow("BUTTON", "Unload PNG", WS_VISIBLE | WS_CHILD, 120, y, 100, 25, hwnd, (HMENU)ID_UNLOAD_PNG, NULL, NULL); y += 35;
            g_hStaticPng = CreateWindow("STATIC", "Default (Procedural)", WS_VISIBLE | WS_CHILD, 10, y, 280, 20, hwnd, NULL, NULL, NULL); y += 40;
            CreateWindow("BUTTON", "Apply & Relaunch", WS_VISIBLE | WS_CHILD | BS_DEFPUSHBUTTON, 10, y, 135, 30, hwnd, (HMENU)ID_APPLY_SETTINGS, NULL, NULL);
            CreateWindow("BUTTON", "Reset Default", WS_VISIBLE | WS_CHILD, 155, y, 125, 30, hwnd, (HMENU)ID_RESET_SETTINGS, NULL, NULL);
            SendMessage(hwnd, WM_USER + 100, 0, 0); 
            break;
        }
        case WM_USER + 100: { 
            SetWindowText(g_hEditZoom, std::to_string(g_Config.zoomFactor).substr(0,4).c_str());
            SetWindowText(g_hEditSize, std::to_string(g_Config.width).c_str());
            SetWindowText(g_hEditOpacity, std::to_string(g_Config.opacity).c_str());
            SetWindowText(g_hEditBorder, std::to_string(g_Config.borderThickness).c_str());
            SetWindowText(g_hEditOffsetX, std::to_string(g_Config.offsetX).c_str());
            SetWindowText(g_hEditOffsetY, std::to_string(g_Config.offsetY).c_str());
            SendMessage(g_hCheckCross, BM_SETCHECK, g_Config.showCrosshair ? BST_CHECKED : BST_UNCHECKED, 0);
            SendMessage(g_hCheckCircle, BM_SETCHECK, g_Config.circularScope ? BST_CHECKED : BST_UNCHECKED, 0);
            SendMessage(g_hCheckNegative, BM_SETCHECK, g_Config.negativeMode ? BST_CHECKED : BST_UNCHECKED, 0);
            SetWindowText(g_hEditCrossSize, std::to_string(g_Config.crosshairSize).c_str());
            if(strlen(g_Config.pngPath) > 0) SetWindowText(g_hStaticPng, g_Config.pngPath);
            else SetWindowText(g_hStaticPng, "Default (Procedural)");
            break;
        }
        case WM_COMMAND: {
            if (LOWORD(wParam) == ID_LOAD_PNG) {
                OPENFILENAMEA ofn = {0}; char szFile[MAX_PATH] = {0};
                ofn.lStructSize = sizeof(ofn); ofn.hwndOwner = hwnd; ofn.lpstrFile = szFile; ofn.nMaxFile = sizeof(szFile);
                ofn.lpstrFilter = "PNG Files\0*.png\0All Files\0*.*\0"; ofn.nFilterIndex = 1;
                ofn.Flags = OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST;
                if (GetOpenFileNameA(&ofn)) { 
                    strcpy_s(g_Config.pngPath, szFile); 
                    SetWindowText(g_hStaticPng, szFile); 
                    LoadCrosshairImage();
                }
            }
            if (LOWORD(wParam) == ID_UNLOAD_PNG) { 
                memset(g_Config.pngPath, 0, MAX_PATH); 
                SetWindowText(g_hStaticPng, "Default (Procedural)"); 
                LoadCrosshairImage(); 
            }
            if (LOWORD(wParam) == ID_APPLY_SETTINGS) {
                char buf[MAX_PATH];
                GetWindowText(g_hEditZoom, buf, 16); g_Config.zoomFactor = (float)atof(buf);
                GetWindowText(g_hEditSize, buf, 16); g_Config.width = g_Config.height = atoi(buf);
                GetWindowText(g_hEditOpacity, buf, 16); g_Config.opacity = atoi(buf);
                GetWindowText(g_hEditBorder, buf, 16); g_Config.borderThickness = atoi(buf);
                GetWindowText(g_hEditOffsetX, buf, 16); g_Config.offsetX = atoi(buf);
                GetWindowText(g_hEditOffsetY, buf, 16); g_Config.offsetY = atoi(buf);
                g_Config.showCrosshair = (SendMessage(g_hCheckCross, BM_GETCHECK, 0, 0) == BST_CHECKED);
                g_Config.circularScope = (SendMessage(g_hCheckCircle, BM_GETCHECK, 0, 0) == BST_CHECKED);
                g_Config.negativeMode = (SendMessage(g_hCheckNegative, BM_GETCHECK, 0, 0) == BST_CHECKED);
                GetWindowText(g_hEditCrossSize, buf, 16); g_Config.crosshairSize = atoi(buf);
                g_Config.Save(); Relaunch();
            }
            if (LOWORD(wParam) == ID_RESET_SETTINGS) { g_Config.Reset(); SendMessage(hwnd, WM_USER + 100, 0, 0); }
            break;
        }
        case WM_CLOSE: ShowWindow(hwnd, SW_HIDE); return 0;
    }
    return DefWindowProc(hwnd, uMsg, wParam, lParam);
}

LRESULT CALLBACK WindowProc(HWND hwnd, UINT uMsg, WPARAM wParam, LPARAM lParam) {
    switch (uMsg) {
        case WM_TRAYICON:
            if (lParam == WM_RBUTTONUP) {
                HMENU hMenu = CreatePopupMenu();
                InsertMenu(hMenu, -1, MF_BYPOSITION, ID_TRAY_SETTINGS, "Settings");
                InsertMenu(hMenu, -1, MF_BYPOSITION, ID_TRAY_EXIT, "Exit");
                POINT pt; GetCursorPos(&pt); SetForegroundWindow(hwnd);
                TrackPopupMenu(hMenu, TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, NULL);
                DestroyMenu(hMenu);
            } else if (lParam == WM_LBUTTONDBLCLK) ShowWindow(g_hwndSettings, SW_SHOW);
            break;
        case WM_COMMAND:
            if (LOWORD(wParam) == ID_TRAY_EXIT) DestroyWindow(hwnd);
            if (LOWORD(wParam) == ID_TRAY_SETTINGS) ShowWindow(g_hwndSettings, SW_SHOW);
            break;
        case WM_DESTROY: Shell_NotifyIcon(NIM_DELETE, &g_nid); PostQuitMessage(0); return 0;
    }
    return DefWindowProc(hwnd, uMsg, wParam, lParam);
}

int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE hPrevInstance, LPSTR lpCmdLine, int nCmdShow) {
    UNREFERENCED_PARAMETER(hPrevInstance); UNREFERENCED_PARAMETER(lpCmdLine); UNREFERENCED_PARAMETER(nCmdShow);
    
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    GdiplusStartupInput gdiplusStartupInput; ULONG_PTR gdiplusToken; GdiplusStartup(&gdiplusToken, &gdiplusStartupInput, NULL);
    g_Config.Load();
    if (!MagInitialize()) return -1;

    LoadCrosshairImage();

    WNDCLASS wc = {0}, swc = {0}, cwc = {0};
    wc.lpfnWndProc = WindowProc; wc.hInstance = hInstance; wc.hCursor = LoadCursor(NULL, IDC_ARROW); wc.lpszClassName = "ZoomToolHost"; RegisterClass(&wc);
    swc.lpfnWndProc = SettingsProc; swc.hInstance = hInstance; swc.hCursor = LoadCursor(NULL, IDC_ARROW); swc.hbrBackground = (HBRUSH)(INT_PTR)(COLOR_WINDOW + 1); swc.lpszClassName = "ZoomSettings"; RegisterClass(&swc);
    cwc.lpfnWndProc = CrosshairProc; cwc.hInstance = hInstance; cwc.hCursor = LoadCursor(NULL, IDC_ARROW); cwc.lpszClassName = "CrosshairWindow"; RegisterClass(&cwc);

    // Create off-screen initially
    g_hwndHost = CreateWindowEx(WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT, "ZoomToolHost", "Precision Zoom", WS_POPUP | WS_CLIPCHILDREN, -2000, -2000, g_Config.width, g_Config.height, NULL, NULL, hInstance, NULL);
    g_hwndCrosshair = CreateWindowEx(WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE, "CrosshairWindow", "Crosshair", WS_POPUP, -2000, -2000, g_Config.width, g_Config.height, NULL, NULL, hInstance, NULL);
    g_hwndSettings = CreateWindow("ZoomSettings", "Zoom Settings", WS_OVERLAPPEDWINDOW & ~WS_MAXIMIZEBOX, 200, 200, 310, 520, NULL, NULL, hInstance, NULL);
    g_hwndMag = CreateWindow(WC_MAGNIFIER, "Mag", WS_CHILD | WS_VISIBLE, 0, 0, g_Config.width, g_Config.height, g_hwndHost, NULL, hInstance, NULL);

    if (strstr(lpCmdLine, "--settings")) {
        ShowWindow(g_hwndSettings, SW_SHOW);
        ForceForegroundWindow(g_hwndSettings);
    }

    if (g_Config.circularScope) {
        HRGN hRgn = CreateEllipticRgn(0, 0, g_Config.width, g_Config.height);
        SetWindowRgn(g_hwndHost, hRgn, TRUE);
        HRGN hRgnC = CreateEllipticRgn(0, 0, g_Config.width, g_Config.height);
        SetWindowRgn(g_hwndCrosshair, hRgnC, TRUE);
    }

    ExcludeToolWindows();
    SetLayeredWindowAttributes(g_hwndCrosshair, TRANSPARENCY_KEY, 255, LWA_COLORKEY);
    SetLayeredWindowAttributes(g_hwndHost, 0, (BYTE)g_Config.opacity, LWA_ALPHA);

    g_nid.cbSize = sizeof(NOTIFYICONDATA); g_nid.hWnd = g_hwndHost; g_nid.uID = 1; g_nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP; g_nid.uCallbackMessage = WM_TRAYICON; g_nid.hIcon = LoadIcon(NULL, IDI_APPLICATION); 
    strcpy_s(g_nid.szTip, "Precision Zoom Tool"); Shell_NotifyIcon(NIM_ADD, &g_nid);
    
    std::thread inputThread(InputPollingThread);
    MSG msg; while (GetMessage(&msg, NULL, 0, 0)) { TranslateMessage(&msg); DispatchMessage(&msg); }
    
    g_Running = false; if (inputThread.joinable()) inputThread.join();
    
    if (g_CachedImage) delete g_CachedImage;
    MagUninitialize(); GdiplusShutdown(gdiplusToken);
    return 0;
}