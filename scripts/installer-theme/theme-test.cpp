// SPDX-License-Identifier: AGPL-3.0-only
#include <iostream>
// clang-format off
#include <windows.h>
#include <commctrl.h>
// clang-format on
#pragma comment(                                                                                   \
    linker,                                                                                        \
    "/manifestdependency:\"type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='x86' publicKeyToken='6595b64144ccf1df' language='*'\"")

extern "C" __declspec(dllimport) void __cdecl Welcome(HWND, int, wchar_t *, void **, void *);
extern "C" __declspec(dllimport) void __cdecl Progress(HWND, int, wchar_t *, void **, void *);
extern "C" __declspec(dllimport) void __cdecl Finish(HWND, int, wchar_t *, void **, void *);

bool require(bool condition, const char *message) {
    if (!condition)
        std::cerr << message << '\n';
    return condition;
}
int main() {
    INITCOMMONCONTROLSEX controls{sizeof(controls), ICC_PROGRESS_CLASS};
    InitCommonControlsEx(&controls);
    bool ok = true;
    for (const double scale : {1.0, 1.25, 1.5, 2.0}) {
        auto parent =
            CreateWindowExW(0, L"STATIC", L"test", WS_OVERLAPPEDWINDOW, 0, 0, int(600 * scale),
                            int(520 * scale), nullptr, nullptr, nullptr, nullptr);
        RECT client{};
        GetClientRect(parent, &client);
        auto create = [&](const wchar_t *kind, int id, int x, int y, int w, int h, HWND owner) {
            return CreateWindowExW(0, kind, L"test", WS_CHILD | WS_VISIBLE, int(x * scale),
                                   int(y * scale), int(w * scale), int(h * scale), owner,
                                   reinterpret_cast<HMENU>(id), nullptr, nullptr);
        };
        const int bottom = int(client.bottom / scale) - 48;
        auto next = create(L"BUTTON", IDOK, 380, bottom, 86, 30, parent);
        auto cancel = create(L"BUTTON", IDCANCEL, 478, bottom, 86, 30, parent);
        auto back = create(L"BUTTON", 3, 294, bottom, 86, 30, parent);
        auto page = create(L"#32770", 1018, 24, 100, 536, 300, parent);
        auto progress = create(PROGRESS_CLASSW, 1004, 0, 20, 500, 20, page);
        create(L"STATIC", 1006, 0, 0, 500, 20, page);
        create(L"BUTTON", 1027, 0, 45, 90, 28, page);
        create(L"LISTBOX", 1016, 0, 80, 500, 200, page);
        ok &= require(parent && next && cancel && back && page && progress, "Create fixture");
        for (auto state : {Welcome, Progress, Finish}) {
            state(parent, 1024, nullptr, nullptr, nullptr);
            SendMessageW(next, BM_SETSTYLE, BS_DEFPUSHBUTTON, TRUE);
            SendMessageW(back, BM_SETSTYLE, BS_PUSHBUTTON, TRUE);
            ShowWindow(back, SW_SHOW);
            RECT a{}, b{}, overlap{};
            GetWindowRect(next, &a);
            GetWindowRect(cancel, &b);
            ok &= require(!IntersectRect(&overlap, &a, &b), "Buttons overlap");
            MapWindowPoints(nullptr, parent, reinterpret_cast<POINT *>(&b), 2);
            ok &= require(b.right <= client.right && b.bottom <= client.bottom, "Button clipped");
            ok &= require((GetWindowLongPtrW(next, GWL_STYLE) & BS_TYPEMASK) == BS_OWNERDRAW,
                          "NSIS reset button theme");
            RECT backBounds{};
            GetWindowRect(back, &backBounds);
            ok &= require(!IntersectRect(&overlap, &a, &backBounds), "Back overlaps next");
            ok &= require((GetWindowLongPtrW(back, GWL_STYLE) & BS_TYPEMASK) == BS_OWNERDRAW,
                          "NSIS reset back button theme");
        }
        SetWindowTextW(next, L"Next");
        wchar_t label[32]{};
        GetWindowTextW(next, label, 32);
        ok &= require(lstrcmpW(label, L"Готово") == 0, "Finish caption overwritten");
        ok &= require(SendMessageW(progress, PBM_GETBARCOLOR, 0, 0) == RGB(19, 164, 237),
                      "Progress isn't CK blue");
        DestroyWindow(parent);
    }
    if (ok)
        std::cout << "Installer layout/theme: 100%, 125%, 150%, 200% passed\n";
    return ok ? 0 : 1;
}
