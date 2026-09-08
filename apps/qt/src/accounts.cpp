#include "window.h"
namespace {
class CapeCard final : public MotionButton {
  public:
    CapeCard(const QString &name, bool none, bool active) : noCape(none), selected(active) {
        setProperty("capeCard", true);
        setAccessibleName(name);
        setCheckable(true);
        setChecked(active);
    }
    void setTexture(const QImage &texture) {
        if (texture.width() >= 64 && texture.height() >= 32) {
            const int scale = texture.width() / 64;
            image = texture.copy(scale, scale, 10 * scale, 16 * scale);
        }
        update();
    }

  protected:
    void paintEvent(QPaintEvent *) override {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        const auto box = QRectF(rect()).adjusted(2, 2, -2, -2);
        QPainterPath clip;
        clip.addRoundedRect(box, 7, 7);
        p.setClipPath(clip);
        p.fillRect(rect(), QColor(22, 41, 58));
        if (!image.isNull()) {
            p.setRenderHint(QPainter::SmoothPixmapTransform, false);
            p.drawImage(box, image);
        } else {
            p.setPen(QColor(123, 205, 240));
            auto f = font();
            f.setPixelSize(22);
            p.setFont(f);
            p.drawText(box.adjusted(0, -10, 0, -10), Qt::AlignCenter, noCape ? s("×") : s("…"));
            if (noCape) {
                f.setPixelSize(10);
                p.setFont(f);
                p.drawText(box.adjusted(0, 42, 0, 0), Qt::AlignCenter, tr("Без плаща"));
            }
        }
        if (underMouse() && isEnabled())
            p.fillRect(rect(), QColor(170, 227, 255, 22));
        p.setClipping(false);
        if (selected || hasFocus()) {
            p.setBrush(Qt::NoBrush);
            p.setPen(QPen(QColor(44, 183, 241), 2));
            p.drawRoundedRect(box, 7, 7);
        }
        if (!isEnabled())
            p.fillPath(clip, QColor(6, 18, 30, 110));
    }

  private:
    QImage image;
    bool noCape, selected;
};
} // namespace
QWidget *LauncherWindow::accountsPage() {
    skinPages = new QStackedWidget;
    auto *gate = new QWidget;
    auto *center = new QVBoxLayout(gate);
    center->setContentsMargins(50, 94, 50, 70);
    auto *login = panel();
    login->setFixedWidth(760);
    login->setObjectName(s("login-card"));
    auto *copy = new QVBoxLayout(login);
    copy->setContentsMargins(34, 32, 34, 32);
    copy->setSpacing(16);
    copy->addWidget(label(tr("ЛИЦЕНЗИОННЫЙ АККАУНТ"), "eyebrow"));
    auto *title = label(tr("Войдите, чтобы продолжить"), "heading");
    title->setWordWrap(true);
    title->setObjectName(s("login-heading"));
    copy->addWidget(title);
    auto *hint =
        label(tr("Откроется безопасная страница входа Microsoft в вашем браузере."), "muted");
    hint->setWordWrap(true);
    copy->addWidget(hint);
    auto *actions = new QHBoxLayout;
    auto *enter = button(tr("Войти через Microsoft"), actions, [this] { signIn(); }, this, true);
    enter->setObjectName(s("microsoft-login"));
    auto *cancelLogin = button(
        tr("Отменить вход"), actions, [this] { core->request(s("cancel_microsoft_login")); }, this);
    cancelLogin->setObjectName(s("cancel-login"));
    cancelLogin->hide();
    actions->addStretch();
    copy->addLayout(actions);
    center->addWidget(login, 0, Qt::AlignHCenter);
    center->addStretch();
    skinPages->addWidget(gate);
    auto *page = new QWidget;
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(40, 44, 44, 24);
    layout->setSpacing(20);
    layout->addWidget(label(tr("Скины и плащи"), "heading"));
    auto *columns = new QHBoxLayout;
    columns->setSpacing(16);
    auto *preview = panel();
    preview->setFixedWidth(320);
    preview->setMinimumHeight(430);
    auto *p = new QVBoxLayout(preview);
    p->setContentsMargins(18, 18, 18, 18);
    p->setSpacing(6);
    auto *badge = label(tr("ТЕКУЩИЙ СКИН"), "badge");
    badge->setObjectName(s("skin-badge"));
    p->addWidget(badge, 0, Qt::AlignLeft);
    skinPreview = new SkinView;
    skinPreview->setObjectName(s("skin-preview"));
    skinPreview->setMinimumHeight(310);
    skinPreview->setAnimated(QSettings().value(s("motion"), true).toBool());
    p->addWidget(skinPreview, 1);
    previewName = label(QString(), "strong");
    previewName->setAlignment(Qt::AlignCenter);
    p->addWidget(previewName);
    auto *applyRow = new QWidget;
    applyRow->setObjectName(s("skin-apply-row"));
    auto *a = new QVBoxLayout(applyRow);
    a->setContentsMargins(0, 8, 0, 0);
    a->setSpacing(8);
    skinVariant = new QComboBox;
    skinVariant->addItem(tr("Классическая модель"), s("classic"));
    skinVariant->addItem(tr("Тонкие руки"), s("slim"));
    a->addWidget(skinVariant);
    button(
        tr("Применить в Minecraft"), a,
        [this] {
            if (!selectedSkin.isEmpty())
                skinAction(s("apply_minecraft_skin"),
                           {{s("accountId"), selectedAccount},
                            {s("skinId"), selectedSkin},
                            {s("variant"), skinVariant->currentData().toString()}});
        },
        this, true);
    connect(skinVariant, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this] { updateSkinPreview(); });
    p->addWidget(applyRow);
    applyRow->hide();
    columns->addWidget(preview, 0, Qt::AlignTop);
    auto *right = new QVBoxLayout;
    right->setSpacing(16);
    auto *library = panel();
    auto *l = new QVBoxLayout(library);
    l->setContentsMargins(20, 20, 20, 20);
    l->setSpacing(16);
    auto *head = new QHBoxLayout;
    auto *titles = new QVBoxLayout;
    titles->setSpacing(4);
    titles->addWidget(label(tr("БИБЛИОТЕКА"), "eyebrow"));
    titles->addWidget(label(tr("Мои скины"), "sectionTitle"));
    head->addLayout(titles, 1);
    addSkinButton = button(tr("+ Добавить PNG"), head, [this] { addSkin(); }, this, true);
    l->addLayout(head);
    auto *filters = new QHBoxLayout;
    skinTabs = new QTabBar;
    skinTabs->setDrawBase(false);
    skinTabs->setObjectName(s("skin-tabs"));
    skinTabs->addTab(tr("Все"));
    skinTabs->addTab(glyph(s("star"), QColor(145, 184, 211), 14), tr("Избранное"));
    skinTabs->setExpanding(false);
    filters->addWidget(skinTabs);
    filters->addStretch();
    skinSearch = new QLineEdit;
    skinSearch->setMaximumWidth(230);
    skinSearch->setPlaceholderText(tr("Поиск по названию"));
    filters->addWidget(skinSearch);
    l->addLayout(filters);
    connect(skinTabs, &QTabBar::currentChanged, this, [this] { renderSkins(); });
    connect(skinSearch, &QLineEdit::textChanged, this, [this] { renderSkins(); });
    skinCards = new CardGrid(174, 296);
    skinCards->setObjectName(s("skin-cards"));
    skinCards->setCardWidth(174);
    l->addWidget(skinCards);
    right->addWidget(library);
    auto *capePanel = panel();
    auto *c = new QVBoxLayout(capePanel);
    c->setContentsMargins(20, 20, 20, 20);
    c->setSpacing(12);
    c->addWidget(label(tr("КОЛЛЕКЦИЯ АККАУНТА"), "eyebrow"));
    c->addWidget(label(tr("Плащи"), "sectionTitle"));
    capeRows = new QVBoxLayout;
    capeRows->setContentsMargins(0, 0, 0, 0);
    c->addLayout(capeRows);
    right->addWidget(capePanel);
    skinStatus = label(QString(), "muted");
    skinStatus->setWordWrap(true);
    right->addWidget(skinStatus);
    right->addStretch();
    columns->addLayout(right, 1);
    layout->addLayout(columns);
    layout->addStretch();
    skinPages->addWidget(scrollPage(page));
    return skinPages;
}
QJsonObject LauncherWindow::currentAccount() const {
    for (const auto &a : accounts)
        if (value(a.toObject(), "id") == selectedAccount)
            return a.toObject();
    return {};
}
void LauncherWindow::accountMenu() {
    auto *popup = new QFrame(this, Qt::Popup | Qt::FramelessWindowHint);
    popup->setObjectName(s("account-popup"));
    popup->setAttribute(Qt::WA_DeleteOnClose);
    popup->setFixedWidth(280);
    auto *layout = new QVBoxLayout(popup);
    layout->setContentsMargins(12, 12, 12, 12);
    layout->setSpacing(8);
    layout->addWidget(label(tr("Аккаунты Minecraft"), "mutedSmall"));
    for (const auto &entry : accounts) {
        const auto account = entry.toObject();
        const auto id = value(account, "id");
        auto *row = new MotionButton;
        row->setProperty("accountRow", true);
        row->setProperty("selected", id == selectedAccount);
        row->setMinimumHeight(58);
        auto *r = new QHBoxLayout(row);
        r->setContentsMargins(8, 6, 8, 6);
        auto *head = new Picture(38);
        head->pixelated = true;
        head->setFallback(value(account, "minecraftName"));
        head->setAttribute(Qt::WA_TransparentForMouseEvents);
        r->addWidget(head);
        images->load(value(account, "headUrl"), head,
                     [head](const QImage &i) { head->setImage(i); });
        auto *name = label(value(account, "minecraftName"), "strong");
        name->setAttribute(Qt::WA_TransparentForMouseEvents);
        r->addWidget(name, 1);
        if (id == selectedAccount)
            r->addWidget(label(QString::fromUtf8("✓"), "accent"));
        layout->addWidget(row);
        connect(row, &QPushButton::clicked, this, [this, popup, id] {
            popup->close();
            if (id != selectedAccount)
                call(
                    s("set_active_account"), {{s("accountId"), id}},
                    [this](const QJsonValue &) { refreshAccounts(); }, true);
        });
    }
    button(
        tr("Добавить аккаунт"), layout,
        [this, popup] {
            popup->close();
            signIn();
        },
        this);
    if (!selectedAccount.isEmpty()) {
        auto *out = button(
            tr("Выйти из аккаунта"), layout,
            [this, popup, id = selectedAccount] {
                popup->close();
                if (QMessageBox::question(
                        this, tr("Выйти из аккаунта?"),
                        tr("Для следующего входа понадобится снова авторизоваться в Microsoft."),
                        QMessageBox::Yes | QMessageBox::No, QMessageBox::No) == QMessageBox::Yes)
                    call(
                        s("remove_account"), {{s("accountId"), id}},
                        [this](const QJsonValue &) { refreshAccounts(); }, true);
            },
            this);
        out->setProperty("danger", true);
    }
    popup->adjustSize();
    auto position =
        accountButton->mapToGlobal(QPoint(accountButton->width() + 8, accountButton->height()));
    position.setY(position.y() - popup->height());
    popup->move(position);
    popup->show();
}
void LauncherWindow::signIn() {
    if (signingIn)
        return;
    signingIn = true;
    auto *enter = findChild<QPushButton *>(s("microsoft-login"));
    auto *cancelLogin = findChild<QPushButton *>(s("cancel-login"));
    enter->setEnabled(false);
    enter->setText(tr("Ожидаем вход…"));
    cancelLogin->show();
    message(tr("Завершите вход в открывшемся браузере."));
    core->request(s("begin_microsoft_login"), {},
                  [this, enter, cancelLogin](const QJsonValue &, const QJsonObject &e) {
                      signingIn = false;
                      enter->setEnabled(true);
                      enter->setText(tr("Войти через Microsoft"));
                      cancelLogin->hide();
                      if (!e.isEmpty()) {
                          message(value(e, "message"), true);
                          return;
                      }
                      refreshAccounts();
                      bringToFront();
                      message(tr("Аккаунт подключён"));
                  });
}
void LauncherWindow::refreshAccounts() {
    call(s("list_accounts"), {}, [this](const QJsonValue &v) {
        accounts = v.toArray();
        const auto previous = selectedAccount;
        selectedAccount.clear();
        for (const auto &entry : accounts)
            if (entry.toObject().value(s("isActive")).toBool())
                selectedAccount = value(entry.toObject(), "id");
        if (previous != selectedAccount) {
            selectedSkin.clear();
            cosmetics = {};
            cosmeticsLoadedAt = 0;
            skins = {};
            ++skinRequest;
            skinPreview->setSkin({});
            skinPreview->setCape({});
        }
        skinPages->setCurrentIndex(selectedAccount.isEmpty() ? 0 : 1);
        accountButton->setIcon(glyph(s("person")));
        const auto account = currentAccount();
        accountButton->setToolTip(selectedAccount.isEmpty() ? tr("Аккаунты Minecraft")
                                                            : value(account, "minecraftName"));
        images->load(value(account, "headUrl"), accountButton,
                     [this, id = selectedAccount](const QImage &i) {
                         if (id == selectedAccount && !i.isNull())
                             accountButton->setIcon(QIcon(QPixmap::fromImage(i).scaled(
                                 38, 38, Qt::KeepAspectRatio, Qt::FastTransformation)));
                     });
        previewName->setText(value(account, "minecraftName"));
        refreshSkins();
        updatePlayState();
    });
}
void LauncherWindow::refreshSkins() {
    if (selectedAccount.isEmpty())
        return;
    const auto id = selectedAccount;
    const auto request = ++skinRequest;
    core->request(s("list_offline_skins"), {{s("accountId"), id}},
                  [this, id, request](const QJsonValue &v, const QJsonObject &e) {
                      if (id != selectedAccount || request != skinRequest)
                          return;
                      if (!e.isEmpty()) {
                          skinStatus->setText(value(e, "message"));
                          return;
                      }
                      skins = v.toArray();
                      bool exists = false;
                      for (const auto &skin : skins)
                          if (value(skin.toObject(), "id") == selectedSkin)
                              exists = true;
                      if (!exists)
                          selectedSkin.clear();
                      renderSkins();
                      updateSkinPreview();
                  });
    if (cosmeticsLoadedAt > 0 && QDateTime::currentMSecsSinceEpoch() - cosmeticsLoadedAt < 30000)
        return;
    core->request(
        s("minecraft_cosmetics"), {{s("accountId"), id}},
        [this, id, request](const QJsonValue &v, const QJsonObject &e) {
            if (id != selectedAccount || request != skinRequest)
                return;
            if (!e.isEmpty()) {
                skinStatus->setText(
                    tr("Не удалось обновить коллекцию аккаунта. Сохранённые скины доступны."));
                return;
            }
            skinStatus->clear();
            cosmetics = v.toObject();
            cosmeticsLoadedAt = QDateTime::currentMSecsSinceEpoch();
            renderSkins();
            renderCapes();
            updateSkinPreview();
        });
}
void LauncherWindow::skinAction(const QString &method, const QJsonObject &params) {
    if (cosmeticPending)
        return;
    cosmeticPending = true;
    const auto id = selectedAccount;
    addSkinButton->setEnabled(false);
    skinStatus->setText(tr("Сохраняем изменения…"));
    core->request(method, params, [this, id](const QJsonValue &, const QJsonObject &e) {
        cosmeticPending = false;
        addSkinButton->setEnabled(true);
        if (id != selectedAccount)
            return;
        if (!e.isEmpty()) {
            skinStatus->setText(value(e, "message"));
            return;
        }
        skinStatus->clear();
        cosmeticsLoadedAt = 0;
        refreshSkins();
    });
}
void LauncherWindow::addSkin() {
    if (!selectedAccount.isEmpty())
        skinAction(s("add_offline_skin"), {{s("accountId"), selectedAccount}});
}

void LauncherWindow::renderSkins() {
    skinCards->clear();
    auto addCard = [this](const QJsonObject &skin, bool online) {
        const auto id = online ? QString() : value(skin, "id"), account = selectedAccount;
        const QString name = online ? tr("Текущий скин") : value(skin, "name");
        if (skinTabs->currentIndex() == 1 && (online || !skin.value(s("isFavorite")).toBool()))
            return;
        if (!skinSearch->text().isEmpty() &&
            !name.contains(skinSearch->text(), Qt::CaseInsensitive))
            return;
        auto *card = new MotionButton;
        card->setObjectName(s("skin-card-") + (online ? s("current") : id));
        card->setProperty("skinCard", true);
        card->setProperty("selected", id == selectedSkin);
        card->setCursor(Qt::PointingHandCursor);
        card->setToolTip(name);
        auto *l = new QVBoxLayout(card);
        l->setContentsMargins(4, 4, 4, 9);
        l->setSpacing(0);
        auto *portrait = new QWidget;
        auto *overlay = new QGridLayout(portrait);
        overlay->setContentsMargins(0, 0, 0, 0);
        auto *view = new SkinView(true);
        overlay->addWidget(view, 0, 0);
        auto *toolWidget = new QWidget;
        auto *tools = new QHBoxLayout(toolWidget);
        tools->setContentsMargins(5, 5, 5, 0);
        overlay->addWidget(toolWidget, 0, 0, Qt::AlignTop);
        if (!online) {
            auto *star = iconButton(s("star"), tr("В избранное"));
            star->setFixedSize(26, 26);
            star->setIconSize(QSize(18, 18));
            if (skin.value(s("isFavorite")).toBool())
                star->setIcon(glyph(s("star"), QColor(255, 211, 87)));
            tools->addWidget(star);
            connect(star, &QPushButton::clicked, this,
                    [this, account, id, favorite = skin.value(s("isFavorite")).toBool()] {
                        skinAction(s("set_offline_skin_favorite"), {{s("accountId"), account},
                                                                    {s("skinId"), id},
                                                                    {s("isFavorite"), !favorite}});
                    });
        }
        tools->addStretch();
        if (id == selectedSkin) {
            auto *check = new QLabel;
            check->setPixmap(glyph(s("check"), QColor(230, 247, 255), 16).pixmap(16, 16));
            check->setFixedSize(22, 22);
            check->setAlignment(Qt::AlignCenter);
            check->setAttribute(Qt::WA_TransparentForMouseEvents);
            tools->addWidget(check);
        }
        if (!online) {
            auto *more = iconButton(s("dots"), tr("Действия со скином"));
            more->setFixedSize(24, 26);
            more->setIconSize(QSize(17, 17));
            tools->addWidget(more);
            connect(more, &QPushButton::clicked, this, [this, account, id, name] {
                QMenu menu(this);
                auto *rename = menu.addAction(tr("Переименовать"));
                auto *remove = menu.addAction(tr("Удалить из библиотеки"));
                auto *chosen = menu.exec(QCursor::pos());
                if (chosen == rename) {
                    bool ok = false;
                    const auto newName =
                        QInputDialog::getText(this, tr("Название скина"), tr("Новое название"),
                                              QLineEdit::Normal, name, &ok);
                    if (ok)
                        skinAction(
                            s("rename_offline_skin"),
                            {{s("accountId"), account}, {s("skinId"), id}, {s("name"), newName}});
                } else if (chosen == remove)
                    skinAction(s("delete_offline_skin"),
                               {{s("accountId"), account}, {s("skinId"), id}});
            });
        }
        l->addWidget(portrait, 1);
        toolWidget->raise();
        const auto url = value(skin, online ? "url" : "dataUrl");
        const bool slim = value(skin, "variant").compare(s("SLIM"), Qt::CaseInsensitive) == 0;
        images->load(url, view, [view, slim](const QImage &i) { view->setSkin(i, slim); });
        auto *caption =
            label(QFontMetrics(card->font()).elidedText(name, Qt::ElideRight, 154), "strong");
        caption->setAlignment(Qt::AlignCenter);
        caption->setFixedHeight(23);
        caption->setAttribute(Qt::WA_TransparentForMouseEvents);
        l->addWidget(caption);
        connect(card, &QPushButton::clicked, this, [this, id, slim] {
            AudioFeedback::play(s("skin-select"));
            selectedSkin = id;
            {
                QSignalBlocker blocker(skinVariant);
                skinVariant->setCurrentIndex(slim ? 1 : 0);
            }
            renderSkins();
            updateSkinPreview();
        });
        skinCards->append(card);
    };
    for (const auto &skin : cosmetics.value(s("skins")).toArray())
        if (value(skin.toObject(), "state") == s("ACTIVE")) {
            addCard(skin.toObject(), true);
            break;
        }
    for (const auto &skin : skins)
        addCard(skin.toObject(), false);
    if (skinTabs->currentIndex() == 0 && skinSearch->text().isEmpty() && skinCards->count() < 4) {
        auto *add = new MotionButton(QString::fromUtf8("+"));
        add->setProperty("addSkin", true);
        add->setToolTip(tr("Добавить PNG-скин"));
        add->setAccessibleName(tr("Добавить PNG-скин"));
        connect(add, &QPushButton::clicked, this, [this] { addSkin(); });
        skinCards->append(add);
    }
    if (!skinCards->count()) {
        auto *empty = label(tr("Скины не найдены"), "muted");
        empty->setAlignment(Qt::AlignCenter);
        empty->setObjectName(s("skin-empty"));
        skinCards->append(empty);
    }
}
void LauncherWindow::updateSkinPreview() {
    const auto account = selectedAccount, id = selectedSkin;
    QString url;
    bool slim = skinVariant->currentData().toString() == s("slim");
    if (id.isEmpty()) {
        for (const auto &skin : cosmetics.value(s("skins")).toArray())
            if (value(skin.toObject(), "state") == s("ACTIVE")) {
                url = value(skin.toObject(), "url");
                slim =
                    value(skin.toObject(), "variant").compare(s("slim"), Qt::CaseInsensitive) == 0;
                break;
            }
    } else
        for (const auto &skin : skins)
            if (value(skin.toObject(), "id") == id)
                url = value(skin.toObject(), "dataUrl");
    findChild<QWidget *>(s("skin-apply-row"))->setVisible(!id.isEmpty());
    findChild<QLabel *>(s("skin-badge"))
        ->setText(id.isEmpty() ? tr("ТЕКУЩИЙ СКИН") : tr("ПРЕДПРОСМОТР"));
    skinPreview->setSkin({});
    images->load(url, skinPreview, [this, account, id, slim](const QImage &i) {
        if (account == selectedAccount && id == selectedSkin &&
            (id.isEmpty() || slim == (skinVariant->currentData().toString() == s("slim"))))
            skinPreview->setSkin(i, slim);
    });
    QString capeUrl;
    for (const auto &cape : cosmetics.value(s("capes")).toArray())
        if (value(cape.toObject(), "state") == s("ACTIVE"))
            capeUrl = value(cape.toObject(), "url");
    skinPreview->setCape({});
    const auto generation = skinRequest;
    images->load(capeUrl, skinPreview, [this, account, generation](const QImage &i) {
        if (account == selectedAccount && generation == skinRequest)
            skinPreview->setCape(i);
    });
}
void LauncherWindow::renderCapes() {
    clearLayout(capeRows);
    auto *grid = new CardGrid(72, 116, 20);
    grid->setCardWidth(72);
    grid->setObjectName(s("cape-grid"));
    capeRows->addWidget(grid);
    const auto list = cosmetics.value(s("capes")).toArray();
    bool hasActive = false;
    for (const auto &cape : list)
        if (value(cape.toObject(), "state") == s("ACTIVE"))
            hasActive = true;
    auto addCape = [this, grid](const QJsonObject &cape, bool none, bool active) {
        auto *card = new CapeCard(none ? tr("Без плаща") : value(cape, "alias"), none, active);
        card->setObjectName(s("cape-") + (none ? s("none") : value(cape, "id")));
        images->load(value(cape, "url"), card,
                     [card](const QImage &image) { card->setTexture(image); });
        grid->append(card);
        connect(card, &QPushButton::clicked, this, [this, id = selectedAccount, cape, none] {
            skinAction(s("activate_minecraft_cape"),
                       {{s("accountId"), id},
                        {s("capeId"),
                         none ? QJsonValue(QJsonValue::Null) : QJsonValue(value(cape, "id"))}});
        });
    };
    addCape({}, true, !hasActive);
    for (const auto &cape : list)
        addCape(cape.toObject(), false, value(cape.toObject(), "state") == s("ACTIVE"));
}
