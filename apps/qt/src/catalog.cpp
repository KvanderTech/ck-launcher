#include "widgets.h"
#include "window.h"
QWidget *LauncherWindow::catalogPage() {
    auto *page = new QWidget;
    auto *layout = pageLayout(page, tr("Найди что-нибудь новое"),
                              tr("Моды, готовые сборки, ресурспаки и шейдеры с Modrinth. "
                                 "Совместимость проверяется перед установкой."));
    auto *search = new QHBoxLayout;
    layout->addLayout(search);
    searchText = new QLineEdit;
    searchText->setPlaceholderText(tr("Название или описание проекта"));
    search->addWidget(searchText, 1);
    searchType = new QComboBox;
    searchType->addItem(tr("Моды"), s("mod"));
    searchType->addItem(tr("Сборки"), s("modpack"));
    searchType->addItem(tr("Ресурспаки"), s("resourcepack"));
    searchType->addItem(tr("Шейдеры"), s("shader"));
    search->addWidget(searchType);
    versionFilter = new QComboBox;
    versionFilter->addItem(tr("Любая версия"));
    search->addWidget(versionFilter);
    button(
        tr("Найти"), search,
        [this] {
            catalogOffset = 0;
            searchCatalog();
        },
        this, true);
    connect(searchText, &QLineEdit::returnPressed, this, [this] {
        catalogOffset = 0;
        searchCatalog();
    });
    catalogStatus =
        new QLabel(tr("Каталог загружается отдельно и не задерживает открытие библиотеки."));
    catalogStatus->setWordWrap(true);
    layout->addWidget(catalogStatus);
    catalogTable = table({tr("Проект"), tr("Автор"), tr("Описание")}, layout);
    catalogTable->setWordWrap(true);
    auto *actions = new QHBoxLayout;
    layout->addLayout(actions);
    button(tr("Установить…"), actions, [this] { installCatalog(); }, this, true);
    button(tr("О проекте"), actions, [this] { projectDetails(); }, this);
    actions->addStretch();
    button(
        tr("← Назад"), actions,
        [this] {
            catalogOffset = qMax(0, catalogOffset - 20);
            searchCatalog();
        },
        this);
    button(
        tr("Далее →"), actions,
        [this] {
            catalogOffset += 20;
            searchCatalog();
        },
        this);
    return page;
}
void LauncherWindow::loadVersions() {
    core->request(
        s("list_game_versions"), {}, [this](const QJsonValue &v, const QJsonObject &error) {
            if (!error.isEmpty()) {
                catalogStatus->setText(tr("Список версий недоступен. Локальная библиотека "
                                          "работает. «Обновить версии» находится в настройках."));
                return;
            }
            versions = {};
            versionFilter->clear();
            versionFilter->addItem(tr("Любая версия"));
            for (const auto &item : v.toArray()) {
                auto version = item.toObject();
                if (value(version, "type") == s("release")) {
                    versions.append(version);
                    versionFilter->addItem(value(version, "id"), value(version, "id"));
                }
            }
        });
}
void LauncherWindow::searchCatalog() {
    const auto request = ++catalogRequest;
    catalog = {};
    catalogTable->setRowCount(0);
    catalogStatus->setText(tr("Ищем проекты…"));
    const QJsonObject params{
        {s("query"), searchText->text()},
        {s("projectType"), searchType->currentData().toString()},
        {s("gameVersion"), versionFilter->currentIndex() > 0
                               ? QJsonValue(versionFilter->currentData().toString())
                               : QJsonValue(QJsonValue::Null)},
        {s("offset"), catalogOffset}};
    core->request(
        s("search_modrinth"), params, [this, request](const QJsonValue &v, const QJsonObject &e) {
            if (request != catalogRequest)
                return;
            if (!e.isEmpty()) {
                catalogStatus->setText(
                    tr("Каталог недоступен. Проверьте подключение и нажмите «Найти» ещё раз."));
                return;
            }
            auto result = v.toObject();
            catalog = result.value(s("hits")).toArray();
            catalogTable->setRowCount(catalog.size());
            for (int r = 0; r < catalog.size(); ++r) {
                auto p = catalog[r].toObject();
                cells(catalogTable, r,
                      {value(p, "title"), value(p, "author"), value(p, "description")});
            }
            catalogStatus->setText(tr("Найдено %1 проектов · страница %2")
                                       .arg(result.value(s("total_hits")).toInt())
                                       .arg(catalogOffset / 20 + 1));
            if (!catalog.isEmpty())
                catalogTable->selectRow(0);
        });
}
void LauncherWindow::projectDetails() {
    int r = catalogTable->currentRow();
    if (r < 0 || r >= catalog.size())
        return;
    auto project = catalog[r].toObject();
    call(s("modrinth_project"), {{s("projectId"), value(project, "project_id")}},
         [this](const QJsonValue &v) {
             QDialog dialog(this);
             dialog.setWindowTitle(value(v.toObject(), "title"));
             dialog.resize(720, 600);
             auto *layout = new QVBoxLayout(&dialog);
             auto *text = new QPlainTextEdit;
             text->setReadOnly(true);
             text->setPlainText(value(v.toObject(), "body"));
             layout->addWidget(text);
             auto *close = new QDialogButtonBox(QDialogButtonBox::Close);
             connect(close, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
             layout->addWidget(close);
             dialog.exec();
         });
}
void LauncherWindow::installCatalog() {
    int r = catalogTable->currentRow();
    if (r < 0 || r >= catalog.size())
        return;
    auto project = catalog[r].toObject();
    const bool pack = value(project, "project_type") == s("modpack");
    if (!pack && selectedBuild.isEmpty()) {
        message(tr("Сначала создайте или выберите сборку в библиотеке."), true);
        return;
    }
    const auto targetBuild = selectedBuild;
    call(
        s("modrinth_project_versions"), {{s("projectId"), value(project, "project_id")}},
        [this, project, pack, targetBuild](const QJsonValue &v) {
            if (!pack && selectedBuild != targetBuild) {
                message(tr("Выбранная сборка изменилась. Повторите установку для нужной сборки."),
                        true);
                return;
            }
            const auto list = v.toArray();
            if (list.isEmpty()) {
                message(tr("Нет доступных версий проекта."), true);
                return;
            }
            QStringList labels;
            for (const auto &item : list) {
                auto version = item.toObject();
                QStringList loaders;
                for (const auto &loader : version.value(s("loaders")).toArray())
                    loaders << loader.toString();
                labels << QString::number(labels.size() + 1) + s(". ") +
                              value(version, "version_number") + s(" · ") + loaders.join(s(", ")) +
                              s(" · ") + value(version, "version_type") + s(" · ") +
                              value(version, "date_published").left(10);
            }
            bool ok;
            auto chosen = QInputDialog::getItem(
                this, tr("Выберите версию проекта"),
                tr("Моды выполняют код на компьютере.\nУстанавливайте проекты, которым доверяете."),
                labels, 0, false, &ok);
            if (!ok)
                return;
            int index = labels.indexOf(chosen);
            QJsonObject params{{s("projectId"), value(project, "project_id")},
                               {s("versionId"), value(list[index].toObject(), "id")}};
            if (!pack)
                params.insert(s("buildId"), targetBuild);
            call(
                pack ? s("install_modrinth_modpack") : s("install_modrinth_project"), params,
                [this](const QJsonValue &) {
                    refreshLibrary();
                    navigation->setCurrentRow(0);
                },
                true);
        });
}
