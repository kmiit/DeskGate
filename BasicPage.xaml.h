#pragma once

#include "BasicPage.g.h"

namespace winrt::DeskGate::implementation
{
    struct BasicPage : BasicPageT<BasicPage>
    {
    public:
        BasicPage();
        
        void ChangeConfig(winrt::Windows::Foundation::IInspectable const& sender, winrt::Microsoft::UI::Xaml::RoutedEventArgs const& e);
    private:
        winrt::Microsoft::UI::Windowing::AppWindow m_mainappwindow{ nullptr };
    };
}
namespace winrt::DeskGate::factory_implementation
{
    struct BasicPage : BasicPageT<BasicPage, implementation::BasicPage>
    {

    };
}
