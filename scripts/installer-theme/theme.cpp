// SPDX-License-Identifier: AGPL-3.0-only
// Small native NSIS plugin: retains real keyboard-accessible Windows buttons.
#include <algorithm>
// clang-format off
#include <windows.h>
#include <commctrl.h>
#include <uxtheme.h>
// clang-format on

namespace {
constexpr COLORREF background = RGB(8, 26, 44);
constexpr COLORREF text = RGB(242, 247, 252);
HWND mainWindow = nullptr;
enum class Page { Neutral, Welcome, Progress, Finish };
Page currentPage = Page::Neutral;

bool drawButton(const DRAWITEMSTRUCT &item) {
    if (item.CtlType != ODT_BUTTON)
        return false;
    const bool primary = item.hwndItem == GetDlgItem(mainWindow, IDOK);
    const bool disabled = (item.itemState & ODS_DISABLED) != 0;
    const bool pressed = (item.itemState & ODS_SELECTED) != 0;
    const COLORREF fill = disabled  ? RGB(23, 49, 67)
                          : primary ? (pressed ? RGB(14, 126, 197) : RGB(19, 164, 237))
                                    : (pressed ? RGB(19, 60, 85) : RGB(16, 46, 70));
    auto bg = CreateSolidBrush(background);
    FillRect(item.hDC, &item.rcItem, bg);
    DeleteObject(bg);
    auto brush = CreateSolidBrush(fill);
    auto pen = CreatePen(PS_SOLID, 1, primary ? fill : RGB(49, 85, 110));
    auto oldBrush = SelectObject(item.hDC, brush);
    auto oldPen = SelectObject(item.hDC, pen);
    const int radius = std::max(8L, (item.rcItem.bottom - item.rcItem.top) / 3);
    RoundRect(item.hDC, item.rcItem.left, item.rcItem.top, item.rcItem.right, item.rcItem.bottom,
              radius, radius);
    SelectObject(item.hDC, oldBrush);
    SelectObject(item.hDC, oldPen);
    DeleteObject(brush);
    DeleteObject(pen);
    wchar_t caption[256]{};
    GetWindowTextW(item.hwndItem, caption, 256);
    const auto font = reinterpret_cast<HFONT>(SendMessageW(item.hwndItem, WM_GETFONT, 0, 0));
    const auto oldFont = SelectObject(item.hDC, font);
    SetBkMode(item.hDC, TRANSPARENT);
    SetTextColor(item.hDC, disabled ? RGB(125, 150, 169) : text);
    RECT rect = item.rcItem;
    if (pressed)
        OffsetRect(&rect, 0, 1);
    DrawTextW(item.hDC, caption, -1, &rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(item.hDC, oldFont);
    if ((item.itemState & ODS_FOCUS) && !(item.itemState & ODS_NOFOCUSRECT)) {
        InflateRect(&rect, -4, -4);
        DrawFocusRect(item.hDC, &rect);
    }
    return true;
}

LRESULT CALLBACK dialogProc(HWND window, UINT message, WPARAM wParam, LPARAM lParam, UINT_PTR id,
                            DWORD_PTR) {
    if (message == WM_DRAWITEM && lParam && drawButton(*reinterpret_cast<DRAWITEMSTRUCT *>(lParam)))
        return TRUE;
    if (message == WM_NCDESTROY)
        RemoveWindowSubclass(window, dialogProc, id);
    return DefSubclassProc(window, message, wParam, lParam);
}

LRESULT CALLBACK buttonProc(HWND window, UINT message, WPARAM wParam, LPARAM lParam, UINT_PTR id,
                            DWORD_PTR) {
    // The NSIS engine reapplies BS_PUSHBUTTON/BS_DEFPUSHBUTTON after page callbacks.
    // Keep native behavior, but don't let that reset our owner-drawn appearance.
    if (message == BM_SETSTYLE)
        wParam = (wParam & ~BS_TYPEMASK) | BS_OWNERDRAW;
    if (window == GetDlgItem(mainWindow, IDOK) && message == WM_SETTEXT) {
        if (currentPage == Page::Welcome)
            lParam = reinterpret_cast<LPARAM>(L"Установить");
        else if (currentPage == Page::Finish)
            lParam = reinterpret_cast<LPARAM>(L"Готово");
    }
    if (message == WM_SETCURSOR && IsWindowEnabled(window)) {
        SetCursor(LoadCursorW(nullptr, IDC_HAND));
        return TRUE;
    }
    if (message == WM_NCDESTROY)
        RemoveWindowSubclass(window, buttonProc, id);
    return DefSubclassProc(window, message, wParam, lParam);
}

LRESULT CALLBACK editProc(HWND window, UINT message, WPARAM wParam, LPARAM lParam, UINT_PTR id,
                          DWORD_PTR) {
    const auto result = DefSubclassProc(window, message, wParam, lParam);
    if (message == WM_NCPAINT) {
        RECT rect{};
        GetWindowRect(window, &rect);
        OffsetRect(&rect, -rect.left, -rect.top);
        auto dc = GetWindowDC(window);
        auto border = CreateSolidBrush(GetFocus() == window ? RGB(19, 164, 237) : RGB(49, 85, 110));
        FrameRect(dc, &rect, border);
        DeleteObject(border);
        ReleaseDC(window, dc);
    }
    if (message == WM_SETFOCUS || message == WM_KILLFOCUS)
        RedrawWindow(window, nullptr, nullptr, RDW_INVALIDATE | RDW_FRAME);
    if (message == WM_NCDESTROY)
        RemoveWindowSubclass(window, editProc, id);
    return result;
}

BOOL CALLBACK styleControl(HWND window, LPARAM) {
    wchar_t className[32]{};
    GetClassNameW(window, className, 32);
    if (lstrcmpiW(className, L"Button") == 0) {
        const auto style = GetWindowLongPtrW(window, GWL_STYLE);
        const auto kind = style & BS_TYPEMASK;
        if (kind != BS_PUSHBUTTON && kind != BS_DEFPUSHBUTTON && kind != BS_OWNERDRAW)
            return TRUE;
        SetWindowLongPtrW(window, GWL_STYLE, (style & ~BS_TYPEMASK) | BS_OWNERDRAW);
        SetWindowSubclass(GetParent(window), dialogProc, 1, 0);
        SetWindowSubclass(window, buttonProc, 1, 0);
        InvalidateRect(window, nullptr, TRUE);
    } else if (lstrcmpiW(className, PROGRESS_CLASSW) == 0) {
        SetWindowTheme(window, L"", L"");
        SetWindowLongPtrW(window, GWL_STYLE,
                          (GetWindowLongPtrW(window, GWL_STYLE) & ~WS_BORDER) | PBS_SMOOTH);
        SendMessageW(window, PBM_SETBARCOLOR, 0, RGB(19, 164, 237));
        SendMessageW(window, PBM_SETBKCOLOR, 0, RGB(16, 46, 70));
    } else if (lstrcmpiW(className, L"Edit") == 0) {
        SetWindowTheme(window, L"", L"");
        SetWindowLongPtrW(window, GWL_EXSTYLE,
                          GetWindowLongPtrW(window, GWL_EXSTYLE) & ~WS_EX_CLIENTEDGE);
        SetWindowLongPtrW(window, GWL_STYLE, GetWindowLongPtrW(window, GWL_STYLE) | WS_BORDER);
        SetWindowSubclass(window, editProc, 1, 0);
        SendMessageW(window, EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN, MAKELPARAM(5, 5));
        SetWindowPos(window, nullptr, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
    return TRUE;
}
} // namespace

// NSIS Unicode plugin ABI. /NOUNLOAD keeps subclass callbacks alive until exit.
extern "C" __declspec(dllexport) void __cdecl Apply(HWND parent, int, wchar_t *, void **, void *) {
    mainWindow = parent;
    EnumChildWindows(parent, styleControl, 0);
    RECT client{}, button{};
    GetClientRect(parent, &client);
    const auto next = GetDlgItem(parent, IDOK);
    const auto cancel = GetDlgItem(parent, IDCANCEL);
    GetWindowRect(cancel, &button);
    MapWindowPoints(nullptr, parent, reinterpret_cast<POINT *>(&button), 2);
    const auto height = button.bottom - button.top;
    const auto width = height * 4;
    const auto gap = height / 2;
    const auto right = client.right - height;
    SetWindowPos(cancel, nullptr, right - width, button.top, width, height,
                 SWP_NOZORDER | SWP_NOACTIVATE);
    SetWindowPos(next, nullptr, right - width * 2 - gap, button.top, width, height,
                 SWP_NOZORDER | SWP_NOACTIVATE);
    SetWindowPos(GetDlgItem(parent, 3), nullptr, height, button.top, width, height,
                 SWP_NOZORDER | SWP_NOACTIVATE);

    // InstallFiles has its own template, separate from our welcome/finish pages.
    // Lay out all of its controls together rather than retaining the tiny stock strip.
    HWND page = FindWindowExW(parent, nullptr, L"#32770", nullptr);
    HWND progress = GetDlgItem(page, 1004);
    if (progress) {
        RECT bounds{};
        GetClientRect(page, &bounds);
        const int margin = int(height / 3), contentWidth = bounds.right - 2 * margin;
        SetWindowPos(GetDlgItem(page, 1006), nullptr, margin, margin, contentWidth, height * 2,
                     SWP_NOZORDER | SWP_NOACTIVATE);
        SetWindowPos(progress, nullptr, margin, height * 2 + margin, contentWidth, height / 3,
                     SWP_NOZORDER | SWP_NOACTIVATE);
        SetWindowPos(GetDlgItem(page, 1027), nullptr, margin, height * 3, width, height,
                     SWP_NOZORDER | SWP_NOACTIVATE);
        SetWindowPos(GetDlgItem(page, 1016), nullptr, margin, height * 4 + margin, contentWidth,
                     std::max(height, bounds.bottom - height * 4 - margin * 2),
                     SWP_NOZORDER | SWP_NOACTIVATE);
    }
    InvalidateRect(parent, nullptr, TRUE);
}

extern "C" __declspec(dllexport) void __cdecl Welcome(HWND parent, int size, wchar_t *vars,
                                                      void **stack, void *extra) {
    currentPage = Page::Welcome;
    Apply(parent, size, vars, stack, extra);
    SetWindowTextW(GetDlgItem(parent, IDOK), L"Установить");
}
extern "C" __declspec(dllexport) void __cdecl Progress(HWND parent, int size, wchar_t *vars,
                                                       void **stack, void *extra) {
    currentPage = Page::Progress;
    Apply(parent, size, vars, stack, extra);
}
extern "C" __declspec(dllexport) void __cdecl Finish(HWND parent, int size, wchar_t *vars,
                                                     void **stack, void *extra) {
    currentPage = Page::Finish;
    Apply(parent, size, vars, stack, extra);
    ShowWindow(GetDlgItem(parent, 3), SW_HIDE);
    ShowWindow(GetDlgItem(parent, IDCANCEL), SW_HIDE);
    SetWindowTextW(GetDlgItem(parent, IDOK), L"Готово");
    EnableMenuItem(GetSystemMenu(parent, FALSE), SC_CLOSE, MF_BYCOMMAND | MF_ENABLED);
}
