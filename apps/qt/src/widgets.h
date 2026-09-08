#pragma once
#include <QJsonObject>
#include <QtWidgets>
#include <functional>
// Animations only run during interaction; geometry stays fixed inside layouts.
class MotionButton : public QPushButton {
  public:
    explicit MotionButton(QWidget *parent = nullptr) : MotionButton(QString(), parent) {}
    explicit MotionButton(const QString &text, QWidget *parent = nullptr)
        : QPushButton(text, parent) {
        setCursor(Qt::PointingHandCursor);
        hoverAnimation.setDuration(170);
        hoverAnimation.setEasingCurve(QEasingCurve::OutCubic);
        pressAnimation.setDuration(100);
        pressAnimation.setEasingCurve(QEasingCurve::OutCubic);
        connect(&hoverAnimation, &QVariantAnimation::valueChanged, this, [this](const QVariant &v) {
            hover = v.toReal();
            update();
        });
        connect(&pressAnimation, &QVariantAnimation::valueChanged, this, [this](const QVariant &v) {
            press = v.toReal();
            update();
        });
    }

    QSize minimumSizeHint() const override {
        auto hint = QPushButton::minimumSizeHint();
        if (property("textButton").toBool())
            hint.setWidth(0);
        return hint;
    }

  protected:
    bool event(QEvent *event) override {
        const bool result = QPushButton::event(event);
        if (event->type() == QEvent::Enter)
            animate(hoverAnimation, hover, 1);
        else if (event->type() == QEvent::Leave) {
            animate(hoverAnimation, hover, 0);
            animate(pressAnimation, press, 0);
        } else if (event->type() == QEvent::MouseButtonPress || event->type() == QEvent::KeyPress)
            animate(pressAnimation, press, isDown() ? 1 : 0);
        else if (event->type() == QEvent::MouseButtonRelease || event->type() == QEvent::KeyRelease)
            animate(pressAnimation, press, 0);
        else if (event->type() == QEvent::EnabledChange && !isEnabled()) {
            hoverAnimation.stop();
            pressAnimation.stop();
            hover = press = 0;
        }
        return result;
    }
    void paintEvent(QPaintEvent *event) override {
        Q_UNUSED(event)
        {
            QStylePainter textPainter(this);
            QStyleOptionButton option;
            initStyleOption(&option);
            // The native dotted focus rectangle clashes with rounded artwork. Keep
            // keyboard focus visible using the rounded ring below instead.
            option.state &= ~QStyle::State_HasFocus;
            if (property("textButton").toBool())
                option.text = textPainter.fontMetrics().elidedText(option.text, Qt::ElideRight,
                                                                   qMax(0, width() - 4));
            textPainter.drawControl(QStyle::CE_PushButton, option);
        }
        if (hasFocus() && keyboardFocus) {
            QPainter focus(this);
            focus.setRenderHint(QPainter::Antialiasing);
            focus.setBrush(Qt::NoBrush);
            focus.setPen(QPen(QColor(141, 223, 255), 1.5));
            focus.drawRoundedRect(QRectF(rect()).adjusted(2, 2, -2, -2), 9, 9);
        }
        if (property("textButton").toBool() || property("capeCard").toBool() || !isEnabled() ||
            (hover < .01 && press < .01))
            return;
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        const QRectF box = QRectF(rect()).adjusted(1, 1, -1, -1);
        const qreal radius = property("iconOnly").toBool()   ? 10
                             : property("skinCard").toBool() ? 18
                                                             : 12;
        QLinearGradient light(0, 0, 0, height());
        light.setColorAt(0, QColor(200, 241, 255, int(28 * hover)));
        light.setColorAt(1, QColor(30, 152, 245, int(10 * hover)));
        p.setBrush(light);
        p.setPen(QPen(QColor(101, 211, 255, int(105 * hover)), 1));
        p.drawRoundedRect(box, radius, radius);
        if (press > .01) {
            p.setPen(Qt::NoPen);
            p.setBrush(QColor(0, 19, 43, int(60 * press)));
            p.drawRoundedRect(box, radius, radius);
        }
    }
    void focusInEvent(QFocusEvent *event) override {
        keyboardFocus = event->reason() == Qt::TabFocusReason ||
                        event->reason() == Qt::BacktabFocusReason ||
                        event->reason() == Qt::ShortcutFocusReason;
        QPushButton::focusInEvent(event);
        update();
    }

  private:
    QVariantAnimation hoverAnimation, pressAnimation;
    qreal hover = 0, press = 0;
    bool keyboardFocus = false;
    void animate(QVariantAnimation &animation, qreal &current, qreal target) {
        animation.stop();
        if (qApp->property("reduceMotion").toBool()) {
            current = target;
            update();
            return;
        }
        animation.setStartValue(current);
        animation.setEndValue(target);
        animation.start();
    }
};
inline QString s(const char *text) {
    return QString::fromUtf8(text);
}
inline QString value(const QJsonObject &object, const char *key) {
    return object.value(s(key)).toString();
}
inline QString baseGameVersion(const QJsonObject &build) {
    const auto version = value(build, "gameVersion"), loader = value(build, "loader"),
               loaderVersion = value(build, "loaderVersion");
    if ((loader == s("fabric") || loader == s("quilt") || loader == s("forge")) &&
        !loaderVersion.isEmpty()) {
        const auto prefix = loader + s("-loader-") + loaderVersion + s("-");
        if (version.startsWith(prefix))
            return version.mid(prefix.size());
    }
    return version;
}
inline QPushButton *button(const QString &title, QBoxLayout *row,
                           const std::function<void()> &action, QObject *context,
                           bool primary = false) {
    auto *b = new MotionButton(title);
    if (primary)
        b->setProperty("primary", true);
    b->setCursor(Qt::PointingHandCursor);
    row->addWidget(b);
    QObject::connect(b, &QPushButton::clicked, context, action);
    return b;
}
inline QTableWidget *table(const QStringList &labels, QVBoxLayout *layout) {
    auto *t = new QTableWidget(0, labels.size());
    t->setHorizontalHeaderLabels(labels);
    t->setSelectionBehavior(QAbstractItemView::SelectRows);
    t->setSelectionMode(QAbstractItemView::SingleSelection);
    t->setEditTriggers(QAbstractItemView::NoEditTriggers);
    t->verticalHeader()->hide();
    t->horizontalHeader()->setStretchLastSection(true);
    t->horizontalHeader()->setSectionResizeMode(QHeaderView::ResizeToContents);
    t->setAlternatingRowColors(true);
    t->setShowGrid(false);
    layout->addWidget(t, 1);
    return t;
}
inline void cells(QTableWidget *table, int row, const QStringList &values) {
    for (int i = 0; i < values.size(); ++i)
        table->setItem(row, i, new QTableWidgetItem(values[i]));
}
inline QVBoxLayout *pageLayout(QWidget *page, const QString &title, const QString &subtitle) {
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(40, 30, 44, 24);
    layout->setSpacing(14);
    auto *heading = new QLabel(title);
    heading->setProperty("heading", true);
    layout->addWidget(heading);
    auto *description = new QLabel(subtitle);
    description->setWordWrap(true);
    description->setProperty("muted", true);
    layout->addWidget(description);
    return layout;
}
