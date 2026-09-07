#include "widgets.h"
#include "window.h"
QWidget *LauncherWindow::accountsPage() {
    auto *page = new QWidget;
    auto *layout = pageLayout(page, tr("Твой профиль"),
                              tr("Вход через системный браузер Microsoft. Токены хранятся в "
                                 "защищённом хранилище Windows."));
    auto *actions = new QHBoxLayout;
    layout->addLayout(actions);
    button(
        tr("Войти через Microsoft"), actions,
        [this] {
            message(tr("Завершите вход в открывшемся браузере."));
            call(s("begin_microsoft_login"), {}, [this](const QJsonValue &) { refreshAccounts(); });
        },
        this, true);
    button(tr("Отменить вход"), actions, [this] { call(s("cancel_microsoft_login")); }, this);
    button(
        tr("Сделать активным"), actions,
        [this] {
            if (!selectedAccount.isEmpty())
                call(s("set_active_account"), {{s("accountId"), selectedAccount}},
                     [this](const QJsonValue &) { refreshAccounts(); });
        },
        this);
    button(
        tr("Удалить аккаунт"), actions,
        [this] {
            if (!selectedAccount.isEmpty() &&
                QMessageBox::question(this, tr("Удалить аккаунт?"),
                                      tr("Для повторного использования понадобится войти снова."),
                                      QMessageBox::Yes | QMessageBox::No,
                                      QMessageBox::No) == QMessageBox::Yes)
                call(s("remove_account"), {{s("accountId"), selectedAccount}},
                     [this](const QJsonValue &) { refreshAccounts(); });
        },
        this);
    accountsTable = table({tr("Игрок"), tr("Статус")}, layout);
    connect(accountsTable, &QTableWidget::itemSelectionChanged, this, [this] {
        int r = accountsTable->currentRow();
        if (r >= 0 && r < accounts.size()) {
            selectedAccount = value(accounts[r].toObject(), "id");
            refreshSkins();
        }
    });
    auto *title = new QLabel(tr("Библиотека скинов"));
    title->setProperty("sectionTitle", true);
    layout->addWidget(title);
    skinsTable = table({tr("Скин"), tr("Выбран"), tr("Избранное")}, layout);
    auto *skinActions = new QHBoxLayout;
    layout->addLayout(skinActions);
    button(
        tr("Добавить PNG"), skinActions,
        [this] {
            if (!selectedAccount.isEmpty())
                call(s("add_offline_skin"), {{s("accountId"), selectedAccount}},
                     [this](const QJsonValue &) { refreshSkins(); });
        },
        this);
    button(
        tr("Применить в Minecraft"), skinActions,
        [this] {
            int r = skinsTable->currentRow();
            if (r < 0 || r >= skins.size())
                return;
            bool ok;
            auto model = QInputDialog::getItem(this, tr("Модель скина"), tr("Ширина рук"),
                                               {s("classic"), s("slim")}, 0, false, &ok);
            if (ok)
                call(s("apply_minecraft_skin"), {{s("accountId"), selectedAccount},
                                                 {s("skinId"), value(skins[r].toObject(), "id")},
                                                 {s("variant"), model}});
        },
        this, true);
    button(
        tr("Избранное"), skinActions,
        [this] {
            int r = skinsTable->currentRow();
            if (r < 0 || r >= skins.size())
                return;
            auto skin = skins[r].toObject();
            call(s("set_offline_skin_favorite"),
                 {{s("accountId"), selectedAccount},
                  {s("skinId"), value(skin, "id")},
                  {s("isFavorite"), !skin.value(s("isFavorite")).toBool()}},
                 [this](const QJsonValue &) { refreshSkins(); });
        },
        this);
    button(
        tr("Удалить"), skinActions,
        [this] {
            int r = skinsTable->currentRow();
            if (r < 0 || r >= skins.size())
                return;
            call(s("delete_offline_skin"),
                 {{s("accountId"), selectedAccount},
                  {s("skinId"), value(skins[r].toObject(), "id")}},
                 [this](const QJsonValue &) { refreshSkins(); });
        },
        this);
    button(
        tr("Плащ…"), skinActions,
        [this] {
            if (selectedAccount.isEmpty())
                return;
            call(s("minecraft_cosmetics"), {{s("accountId"), selectedAccount}},
                 [this](const QJsonValue &v) {
                     auto capes = v.toObject().value(s("capes")).toArray();
                     QStringList labels{tr("Без плаща")};
                     for (const auto &cape : capes)
                         labels << value(cape.toObject(), "alias");
                     bool ok;
                     auto name = QInputDialog::getItem(
                         this, tr("Плащ Minecraft"), tr("Доступные плащи"), labels, 0, false, &ok);
                     if (ok) {
                         int i = labels.indexOf(name);
                         call(s("activate_minecraft_cape"),
                              {{s("accountId"), selectedAccount},
                               {s("capeId"),
                                i == 0 ? QJsonValue(QJsonValue::Null)
                                       : QJsonValue(value(capes[i - 1].toObject(), "id"))}});
                     }
                 });
        },
        this);
    return page;
}
void LauncherWindow::refreshAccounts() {
    call(s("list_accounts"), {}, [this](const QJsonValue &v) {
        accounts = v.toArray();
        QSignalBlocker block(accountsTable);
        accountsTable->setRowCount(accounts.size());
        accountLabel->setText(tr("Войдите в Microsoft\nдля запуска игры"));
        for (int r = 0; r < accounts.size(); ++r) {
            auto a = accounts[r].toObject();
            bool active = a.value(s("isActive")).toBool();
            cells(accountsTable, r,
                  {value(a, "minecraftName"), active ? tr("Активный") : tr("Сохранён")});
            if (active) {
                accountLabel->setText(value(a, "minecraftName"));
                selectedAccount = value(a, "id");
                accountsTable->selectRow(r);
            }
        }
        if (accounts.isEmpty())
            selectedAccount.clear();
        refreshSkins();
    });
}
void LauncherWindow::refreshSkins() {
    if (selectedAccount.isEmpty()) {
        skinsTable->setRowCount(0);
        return;
    }
    const auto id = selectedAccount;
    call(s("list_offline_skins"), {{s("accountId"), id}}, [this, id](const QJsonValue &v) {
        if (id != selectedAccount)
            return;
        skins = v.toArray();
        skinsTable->setRowCount(skins.size());
        for (int r = 0; r < skins.size(); ++r) {
            auto skin = skins[r].toObject();
            cells(skinsTable, r,
                  {value(skin, "name"), skin.value(s("isActive")).toBool() ? tr("Да") : QString(),
                   skin.value(s("isFavorite")).toBool() ? tr("★") : QString()});
        }
    });
}
